//! Recoverable, exclusively locked publication of complete generated file sets.
//!
//! Callers validate the candidate before committing. Publication is recoverable, not
//! globally atomic: readers can observe intermediate files during promotion. The root
//! must be trusted against concurrent directory/symlink replacement by other processes.
//! Cooperative writers are serialized; unrelated edits are detected by digest checks.

#![deny(missing_docs)]

pub mod migration;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sourcefield_io::{copy_sha256, read_json_bounded, sha256_reader};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CONTROL: &str = ".sourcefield-transaction";
const LOCK: &str = ".sourcefield-lock";
const OWNERSHIP: &str = ".sourcefield-owned.json";
const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;

/// Files owned by a successfully published generator run.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnershipManifest {
    /// Independent ownership manifest format version.
    pub schema_version: u32,
    /// Relative file paths and the SHA-256 digest of their published bytes.
    pub files: BTreeMap<String, String>,
    /// Authored files updated in this candidate; omission never authorizes deletion.
    #[serde(default)]
    pub authored_files: BTreeMap<String, String>,
}

/// Summary of an installed complete candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitReport {
    /// Files published, excluding internal transaction metadata.
    pub written: usize,
    /// Previously owned files removed because the new candidate omits them.
    pub removed: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    candidate_manifest_sha256: String,
    entries: BTreeMap<String, Option<String>>,
}

/// An exclusive candidate builder whose drop releases its cooperative writer lock.
///
/// Staging a generated file adopts its ownership explicitly. Existing unowned files
/// require `stage_checked`, preventing accidental adoption of authored files.
/// A successful commit relinquishes omitted previously owned paths.
#[derive(Debug)]
pub struct Transaction {
    root: PathBuf,
    candidate: PathBuf,
    previous: OwnershipManifest,
    staged: BTreeMap<String, String>,
    authored: BTreeSet<String>,
    expected: BTreeMap<String, Option<String>>,
    _lock: LockGuard,
    prepared: bool,
}

/// Exclusive lock file; the token prevents deleting a replacement lock on drop.
#[derive(Debug)]
struct LockGuard {
    path: PathBuf,
    token: String,
}

impl LockGuard {
    fn acquire(root: &Path) -> Result<Self> {
        let path = root.join(LOCK);
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .context(
                "workspace is locked; inspect the owner before recovering an abandoned lock",
            )?;

        let token = format!(
            "{}:{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );

        file.write_all(token.as_bytes())?;
        file.sync_all()?;

        Ok(Self { path, token })
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path).ok().as_deref() == Some(self.token.as_str()) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Transaction {
    /// Acquire the root lock and create an empty candidate after recovering prior work.
    /// The root must resolve through real directories or recognized OS-owned temporary aliases.
    pub fn begin(root: impl AsRef<Path>) -> Result<Self> {
        let root = checked_workspace_root(root.as_ref())?;
        let lock = LockGuard::acquire(&root)?;

        recover_locked(&root)?;

        let previous = read_ownership(&root)?;
        let mut expected = BTreeMap::new();

        // Missing tracked-but-ignored output is an observed precondition, not a modification.
        // Capture omissions too so a concurrent appearance cannot become deletion authority.
        for key in previous.files.keys() {
            expected.insert(key.clone(), digest_optional(&safe_path(&root, key)?)?);
        }

        let candidate = root.join(CONTROL).join("candidate");

        fs::create_dir(root.join(CONTROL))?;
        fs::create_dir(&candidate)?;

        Ok(Self {
            root,
            candidate,
            previous,
            staged: BTreeMap::new(),
            authored: BTreeSet::new(),
            expected,
            _lock: lock,
            prepared: false,
        })
    }

    /// Candidate tree available for read-only validation by renderer/browser checks.
    pub fn candidate_dir(&self) -> &Path {
        &self.candidate
    }

    /// Stage bytes as a generated file, rejecting unowned existing destinations.
    pub fn stage(&mut self, relative: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
        self.stage_reader(relative, bytes)
    }

    /// Stream a generated file into the candidate with bounded buffer memory.
    pub fn stage_reader(&mut self, relative: impl AsRef<Path>, reader: impl Read) -> Result<()> {
        let key = relative_key(relative.as_ref())?;
        let current = digest_optional(&safe_path(&self.root, &key)?)?;

        if let Some(previous) = self.previous.files.get(&key) {
            ensure!(
                current.as_ref().is_none_or(|digest| digest == previous),
                "owned file was modified: {key}"
            );
        } else {
            ensure!(
                current.is_none(),
                "existing unowned file requires an explicit precondition: {key}"
            );
        }

        if let Some(observed) = self.expected.get(&key) {
            ensure!(
                &current == observed,
                "owned file changed after transaction began: {key}"
            );
        }

        self.stage_inner(key, reader, current)
    }

    /// Adopt a generated file after comparing its old digest.
    /// Use `stage_authored` for READMEs whose omission must never authorize deletion.
    /// `None` requires absence. The digest is checked again immediately before promotion.
    pub fn stage_checked(
        &mut self,
        relative: impl AsRef<Path>,
        bytes: &[u8],
        expected_sha256: Option<&str>,
    ) -> Result<()> {
        let key = relative_key(relative.as_ref())?;
        let current = digest_optional(&safe_path(&self.root, &key)?)?;

        ensure!(
            current.as_deref() == expected_sha256,
            "source precondition failed: {key}"
        );

        self.stage_inner(key, bytes, current)
    }

    /// Stage an authored file transactionally without granting deletion ownership.
    ///
    /// This is the managed-README API: an omitted README remains untouched in later
    /// runs. Its current digest must match the caller's observed source, permitting
    /// deliberate regeneration around independently edited authored paragraphs.
    pub fn stage_authored(
        &mut self,
        relative: impl AsRef<Path>,
        bytes: &[u8],
        expected_sha256: Option<&str>,
    ) -> Result<()> {
        let key = relative_key(relative.as_ref())?;

        self.stage_checked(&key, bytes, expected_sha256)?;
        self.authored.insert(key);

        Ok(())
    }

    fn stage_inner(
        &mut self,
        key: String,
        reader: impl Read,
        expected: Option<String>,
    ) -> Result<()> {
        ensure!(!self.prepared, "candidate is already prepared");
        ensure!(
            !self.staged.contains_key(&key),
            "duplicate staged path: {key}"
        );

        let path = safe_path(&self.candidate, &key)?;

        create_parent(&path)?;

        let digest = copy_digest(reader, &path)?;

        self.expected.insert(key.clone(), expected);
        self.staged.insert(key, digest);

        Ok(())
    }

    /// Check staged bytes, preconditions, and every omitted owned path before mutation.
    /// This supplements domain validation; it does not validate SVG or README semantics.
    pub fn validate(&self) -> Result<()> {
        for (key, expected) in &self.expected {
            ensure!(
                digest_optional(&safe_path(&self.root, key)?)? == *expected,
                "source changed during generation: {key}"
            );
        }

        for (key, digest) in &self.staged {
            ensure!(
                file_sha256(safe_path(&self.candidate, key)?)? == *digest,
                "candidate changed after staging: {key}"
            );
        }

        for (key, digest) in &self.previous.files {
            if !self.staged.contains_key(key) {
                ensure!(
                    self.expected
                        .get(key)
                        .and_then(Option::as_ref)
                        .is_none_or(|observed| observed == digest),
                    "stale owned file was modified: {key}"
                );
            }
        }

        Ok(())
    }

    /// Validate and publish the complete file set, retaining recovery backups on failure.
    /// All input files must already have passed caller-owned semantic validation.
    pub fn commit(mut self) -> Result<CommitReport> {
        self.prepare()?;
        self.promote(None)
    }

    /// Make all recovery copies durable before recording permission to mutate output.
    fn prepare(&mut self) -> Result<()> {
        self.validate()?;

        let control = self.root.join(CONTROL);
        let backup = control.join("backup");
        let mut entries = self.expected.clone();

        entries.insert(
            OWNERSHIP.to_owned(),
            digest_optional(&safe_internal_path(&self.root, OWNERSHIP)?)?,
        );
        fs::create_dir(&backup)?;

        for (key, digest) in &entries {
            if let Some(digest) = digest {
                let original = safe_internal_path(&self.root, key)?;
                let target = safe_internal_path(&backup, key)?;

                create_parent(&target)?;
                copy_digest(File::open(original)?, &target)?;
                ensure!(
                    file_sha256(&target)? == *digest,
                    "source changed while backing up: {key}"
                );
            }
        }

        let manifest = OwnershipManifest {
            schema_version: 1,
            files: self
                .staged
                .iter()
                .filter(|(key, _)| !self.authored.contains(*key))
                .map(|(key, digest)| (key.clone(), digest.clone()))
                .collect(),
            authored_files: self
                .staged
                .iter()
                .filter(|(key, _)| self.authored.contains(*key))
                .map(|(key, digest)| (key.clone(), digest.clone()))
                .collect(),
        };

        write_json(&self.candidate.join(OWNERSHIP), &manifest)?;

        let journal = Journal {
            schema_version: 1,
            candidate_manifest_sha256: file_sha256(self.candidate.join(OWNERSHIP))?,
            entries,
        };

        self.validate()?;
        write_json(&control.join("journal.pending"), &journal)?;
        fs::rename(
            control.join("journal.pending"),
            control.join("journal.json"),
        )?;
        sync_directory(&control)?;
        self.prepared = true;

        Ok(())
    }

    fn promote(&mut self, fail_after: Option<usize>) -> Result<CommitReport> {
        let mut count = 0;
        let mut removed = 0;

        for key in self
            .staged
            .keys()
            .chain(std::iter::once(&OWNERSHIP.to_owned()))
        {
            let source = safe_internal_path(&self.candidate, key)?;
            let destination = safe_internal_path(&self.root, key)?;

            install_file(&source, &destination)?;
            count += 1;

            if fail_after == Some(count) {
                bail!("injected promotion failure");
            }
        }

        for key in self.previous.files.keys() {
            if !self.staged.contains_key(key) && self.expected.get(key).is_some_and(Option::is_some)
            {
                fs::remove_file(safe_path(&self.root, key)?)?;
                removed += 1;
            }
        }

        // A durable marker distinguishes committed output from a partially applied journal.
        copy_digest(&b"complete"[..], &self.root.join(CONTROL).join("committed"))?;
        sync_directory(&self.root.join(CONTROL))?;
        fs::remove_dir_all(self.root.join(CONTROL))?;
        sync_directory(&self.root)?;

        Ok(CommitReport {
            written: self.staged.len(),
            removed,
        })
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        // Before a journal exists no public output was touched; incomplete candidates are disposable.
        if !self.prepared && !self.root.join(CONTROL).join("journal.json").exists() {
            let _ = fs::remove_dir_all(self.root.join(CONTROL));
        }
    }
}

/// Recover interrupted promotion under the same exclusive lock used by writers.
/// Live or abandoned lock files are never stolen automatically.
pub fn recover(root: impl AsRef<Path>) -> Result<()> {
    let root = checked_workspace_root(root.as_ref())?;
    let _lock = LockGuard::acquire(&root)?;

    recover_locked(&root)
}

/// Remove a crash-abandoned lock after the operator has confirmed its process is stopped.
///
/// The exact recorded token is required. Never call this while a writer may still run:
/// process liveness cannot be established portably from a PID (which may be reused).
pub fn release_abandoned_lock(root: impl AsRef<Path>, expected_token: &str) -> Result<()> {
    let root = checked_workspace_root(root.as_ref())?;
    let path = safe_internal_path(&root, LOCK)?;

    ensure!(
        fs::read_to_string(&path)? == expected_token,
        "lock owner changed; recovery refused"
    );
    fs::remove_file(path)?;

    Ok(())
}

fn recover_locked(root: &Path) -> Result<()> {
    let control = safe_internal_path(root, CONTROL)?;

    if !control.exists() {
        return Ok(());
    }

    reject_tree_symlinks(&control)?;

    if !control.join("committed").exists() && control.join("journal.json").exists() {
        let journal: Journal = read_json(&control.join("journal.json"))?;

        ensure!(
            journal.schema_version == 1,
            "unsupported recovery journal version"
        );

        validate_recovery_inventory(&control, &journal)?;

        // Validate every backup before restoring any file, retaining evidence on corruption.
        for (key, digest) in &journal.entries {
            if key != OWNERSHIP {
                relative_key(Path::new(key))?;
            }

            safe_internal_path(root, key)?;

            if let Some(digest) = digest {
                ensure!(
                    file_sha256(safe_internal_path(&control.join("backup"), key)?)? == *digest,
                    "recovery backup digest mismatch: {key}"
                );
            }
        }

        for (key, digest) in &journal.entries {
            validate_recovery_destination(root, &control, key, digest.as_deref())?;
        }

        for (key, digest) in &journal.entries {
            let target = safe_internal_path(root, key)?;

            if digest.is_some() {
                install_file(&safe_internal_path(&control.join("backup"), key)?, &target)?;
            } else if target.exists() {
                ensure!(
                    target.is_file(),
                    "recovery destination is not a file: {key}"
                );
                fs::remove_file(target)?;
            }
        }
    }

    fs::remove_dir_all(control)?;
    sync_directory(root)?;

    Ok(())
}

/// Preserve independent edits made after a failed run instead of rolling them back silently.
fn validate_recovery_destination(
    root: &Path,
    control: &Path,
    key: &str,
    original_digest: Option<&str>,
) -> Result<()> {
    let target = safe_internal_path(root, key)?;
    let current = digest_optional(&target)?;

    if current.is_none() || current.as_deref() == original_digest {
        return Ok(());
    }

    let candidate = safe_internal_path(&control.join("candidate"), key)?;

    ensure!(
        candidate.is_file() && is_file_prefix(&target, &candidate)?,
        "destination independently modified since interrupted publication: {key}"
    );

    Ok(())
}

/// Interrupted truncating copies can only leave a prefix of the validated candidate.
fn is_file_prefix(target: &Path, candidate: &Path) -> Result<bool> {
    let target_length = fs::metadata(target)?.len();

    if target_length > fs::metadata(candidate)?.len() {
        return Ok(false);
    }

    let mut target = File::open(target)?;
    let mut candidate = File::open(candidate)?;
    let mut remaining = target_length;
    let mut target_buffer = [0_u8; 64 * 1024];
    let mut candidate_buffer = [0_u8; 64 * 1024];

    while remaining > 0 {
        let length = remaining.min(target_buffer.len() as u64) as usize;

        target.read_exact(&mut target_buffer[..length])?;
        candidate.read_exact(&mut candidate_buffer[..length])?;

        if target_buffer[..length] != candidate_buffer[..length] {
            return Ok(false);
        }

        remaining -= length as u64;
    }

    Ok(true)
}

/// Reconcile journal destinations against the two ownership inventories before restoration.
fn validate_recovery_inventory(control: &Path, journal: &Journal) -> Result<()> {
    let candidate_path = control.join("candidate").join(OWNERSHIP);

    ensure!(
        file_sha256(&candidate_path)? == journal.candidate_manifest_sha256,
        "candidate ownership manifest was modified"
    );

    let candidate: OwnershipManifest = read_json(&candidate_path)?;
    let previous = if let Some(Some(digest)) = journal.entries.get(OWNERSHIP) {
        let path = control.join("backup").join(OWNERSHIP);

        ensure!(
            file_sha256(&path)? == *digest,
            "previous ownership backup was modified"
        );

        read_json::<OwnershipManifest>(&path)?
    } else {
        OwnershipManifest {
            schema_version: 1,
            files: BTreeMap::new(),
            authored_files: BTreeMap::new(),
        }
    };

    ensure!(
        candidate.schema_version == 1 && previous.schema_version == 1,
        "unsupported recovery ownership version"
    );

    let mut expected: std::collections::BTreeSet<&str> = candidate
        .files
        .keys()
        .chain(candidate.authored_files.keys())
        .chain(previous.files.keys())
        .map(String::as_str)
        .collect();

    expected.insert(OWNERSHIP);
    ensure!(
        journal
            .entries
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>()
            == expected,
        "journal destinations do not match ownership inventories"
    );

    Ok(())
}

/// Calculate SHA-256 using a fixed-size buffer, independent of file size.
pub fn file_sha256(path: impl AsRef<Path>) -> Result<String> {
    Ok(sha256_reader(File::open(path.as_ref())?)?)
}

fn digest_optional(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "expected regular file: {}",
                path.display()
            );

            Ok(Some(file_sha256(path)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Validate a caller-selected existing directory before canonicalizing its path.
/// Relative paths resolve from the working directory; only root-owned macOS `/tmp`
/// and `/var` aliases may traverse symlinks. Descendant and custom symlinks fail.
/// This cooperative filesystem boundary does not prevent concurrent hostile path replacement.
pub fn checked_workspace_root(root: &Path) -> Result<PathBuf> {
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()?.join(root)
    };

    // Only OS-owned macOS aliases receive an exception; caller-created symlinks remain forbidden.
    for ancestor in absolute
        .ancestors()
        .filter(|path| !path.as_os_str().is_empty())
    {
        let metadata = fs::symlink_metadata(ancestor)
            .with_context(|| format!("inspect workspace root component {}", ancestor.display()))?;

        ensure!(
            !metadata.file_type().is_symlink() || trusted_system_alias(ancestor)?,
            "symlink in workspace root component: {}",
            ancestor.display()
        );
    }

    let canonical = absolute
        .canonicalize()
        .with_context(|| format!("canonicalize workspace root {}", absolute.display()))?;

    ensure!(
        fs::metadata(&canonical)?.is_dir(),
        "workspace root must be a real directory: {}",
        canonical.display()
    );

    Ok(canonical)
}

/// macOS exposes its root-owned temporary directories through these documented filesystem aliases.
fn trusted_system_alias(path: &Path) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let expected = match path.to_str() {
            Some("/tmp") => Path::new("/private/tmp"),
            Some("/var") => Path::new("/private/var"),
            _ => return Ok(false),
        };

        use std::os::unix::fs::MetadataExt;

        let metadata = fs::symlink_metadata(path)?;
        let target = fs::read_link(path)?;
        let absolute_target = if target.is_absolute() {
            target
        } else {
            Path::new("/").join(target)
        };

        Ok(metadata.uid() == 0 && absolute_target == expected)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;

        Ok(false)
    }
}

fn relative_key(path: &Path) -> Result<String> {
    let key = normalized_key(path)?;

    ensure!(
        !key.split('/')
            .any(|part| matches!(part, CONTROL | LOCK | OWNERSHIP | ".git")),
        "reserved destination: {key}"
    );

    Ok(key)
}

fn normalized_key(path: &Path) -> Result<String> {
    ensure!(!path.as_os_str().is_empty(), "empty relative path");

    let mut parts = Vec::new();

    for part in path.components() {
        match part {
            Component::Normal(value) => {
                let value = value.to_str().context("non-UTF-8 output path")?;

                ensure!(
                    !value.contains(['\\', ':']) && value != "." && !value.ends_with(['.', ' ']),
                    "nonportable path component"
                );
                let stem = value
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_uppercase();

                let reserved_device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                        && stem.len() == 4
                        && matches!(stem.as_bytes()[3], b'1'..=b'9'));

                ensure!(
                    !reserved_device
                        && !value.chars().any(
                            |character| character.is_control() || "<>\"|?*".contains(character)
                        ),
                    "nonportable device or special path component"
                );
                parts.push(value);
            }
            _ => bail!("output paths must be relative without traversal"),
        }
    }

    ensure!(!parts.is_empty(), "empty relative path");

    Ok(parts.join("/"))
}

fn safe_path(root: &Path, key: &str) -> Result<PathBuf> {
    relative_key(Path::new(key))?;
    safe_internal_path(root, key)
}

fn safe_internal_path(root: &Path, key: &str) -> Result<PathBuf> {
    let key = normalized_key(Path::new(key))?;
    let mut path = root.to_path_buf();

    for part in key.split('/') {
        path.push(part);

        match fs::symlink_metadata(&path) {
            Ok(metadata) => ensure!(
                !metadata.file_type().is_symlink(),
                "symlink output boundary: {}",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    Ok(path)
}

fn create_parent(path: &Path) -> Result<()> {
    fs::create_dir_all(path.parent().context("file has no parent")?)?;

    Ok(())
}

fn copy_digest(reader: impl Read, destination: &Path) -> Result<String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let digest = copy_sha256(reader, &mut file)?;

    file.sync_all()?;

    Ok(digest)
}

fn install_file(source: &Path, destination: &Path) -> Result<()> {
    create_parent(destination)?;

    // Copy directly under the journal: Windows rename cannot replace an existing file.
    // Backups remain intact so interruption during this write is recoverable everywhere.
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(destination)?;

    std::io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    sync_directory(destination.parent().context("missing parent")?)?;

    Ok(())
}

fn read_ownership(root: &Path) -> Result<OwnershipManifest> {
    let path = safe_internal_path(root, OWNERSHIP)?;

    if !path.exists() {
        return Ok(OwnershipManifest {
            schema_version: 1,
            files: BTreeMap::new(),
            authored_files: BTreeMap::new(),
        });
    }

    let manifest: OwnershipManifest = read_json(&path)?;

    ensure!(
        manifest.schema_version == 1,
        "unsupported ownership manifest version"
    );

    for key in manifest.files.keys().chain(manifest.authored_files.keys()) {
        relative_key(Path::new(key))?;
    }

    Ok(manifest)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path)?;

    ensure!(
        file.metadata()?.len() <= MAX_METADATA_BYTES,
        "metadata exceeds size limit"
    );

    read_json_bounded(file, MAX_METADATA_BYTES).context("read bounded transaction metadata")
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;

    ensure!(
        bytes.len() as u64 <= MAX_METADATA_BYTES,
        "metadata exceeds size limit"
    );
    copy_digest(bytes.as_slice(), path)?;

    Ok(())
}

fn reject_tree_symlinks(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;

        ensure!(
            !metadata.file_type().is_symlink(),
            "symlink in transaction tree"
        );

        if metadata.is_dir() {
            reject_tree_symlinks(&entry.path())?;
        }
    }

    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;

    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    // Windows std cannot open directory handles for FlushFileBuffers. File contents
    // are synced, but filesystem/power-loss directory durability is platform-dependent.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    pub(crate) struct TestRoot(pub(crate) PathBuf);

    impl TestRoot {
        pub(crate) fn new() -> Self {
            let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "sourcefield-workspace-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));

            fs::create_dir(&path).unwrap();

            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Arrange a journaled interruption so each recovery test has a single recovery act.
    fn interrupt_publication(transaction: &mut Transaction, after: usize) {
        transaction.prepare().unwrap();
        let error = transaction.promote(Some(after)).unwrap_err();

        assert_eq!(error.to_string(), "injected promotion failure");
    }

    #[test]
    fn publishes_complete_candidate() {
        let root = TestRoot::new();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction.stage("docs/state.json", b"state").unwrap();
        transaction.stage("docs/app.js", b"runtime").unwrap();

        let report = transaction.commit().unwrap();

        assert_eq!(
            report,
            CommitReport {
                written: 2,
                removed: 0
            }
        );
        assert_eq!(fs::read(root.0.join("docs/state.json")).unwrap(), b"state");
        assert!(!root.0.join(LOCK).exists());
    }

    #[test]
    fn concurrent_writer_and_recovery_are_rejected() {
        let root = TestRoot::new();
        let _first = Transaction::begin(&root.0).unwrap();

        let writer = Transaction::begin(&root.0);
        let recovery = recover(&root.0);

        assert!(writer.is_err());
        assert!(recovery.is_err());
    }

    #[test]
    fn modified_authored_file_prevents_all_publication() {
        let root = TestRoot::new();
        fs::write(root.0.join("README.md"), b"before").unwrap();
        let digest = file_sha256(root.0.join("README.md")).unwrap();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction
            .stage_checked("README.md", b"generated", Some(&digest))
            .unwrap();
        transaction.stage("docs/new.svg", b"svg").unwrap();
        fs::write(root.0.join("README.md"), b"user edit").unwrap();

        let result = transaction.commit();

        assert!(result.is_err());
        assert_eq!(fs::read(root.0.join("README.md")).unwrap(), b"user edit");
        assert!(!root.0.join("docs/new.svg").exists());
    }

    #[test]
    fn upgrade_removes_only_previously_owned_files() {
        let root = TestRoot::new();
        let mut first = Transaction::begin(&root.0).unwrap();
        first.stage("old.js", b"old").unwrap();
        first.commit().unwrap();
        fs::write(root.0.join("authored.txt"), b"keep").unwrap();
        let mut next = Transaction::begin(&root.0).unwrap();
        next.stage("new.js", b"new").unwrap();

        let report = next.commit().unwrap();

        assert_eq!(report.removed, 1);
        assert!(!root.0.join("old.js").exists());
        assert_eq!(fs::read(root.0.join("authored.txt")).unwrap(), b"keep");
    }

    #[test]
    fn refuses_modified_stale_owned_file() {
        let root = TestRoot::new();
        let mut first = Transaction::begin(&root.0).unwrap();
        first.stage("old.js", b"old").unwrap();
        first.commit().unwrap();
        fs::write(root.0.join("old.js"), b"user changes").unwrap();
        let next = Transaction::begin(&root.0).unwrap();

        let result = next.commit();

        assert!(result.is_err());
        assert_eq!(fs::read(root.0.join("old.js")).unwrap(), b"user changes");
    }

    #[test]
    fn recovers_interrupted_promotion_and_keeps_original_runtime() {
        let root = TestRoot::new();
        fs::write(root.0.join("a.js"), b"original runtime").unwrap();
        let digest = file_sha256(root.0.join("a.js")).unwrap();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction
            .stage_checked("a.js", b"new runtime", Some(&digest))
            .unwrap();
        transaction.stage("b.json", b"new schema").unwrap();
        interrupt_publication(&mut transaction, 1);
        drop(transaction);

        recover(&root.0).unwrap();

        assert_eq!(fs::read(root.0.join("a.js")).unwrap(), b"original runtime");
        assert!(!root.0.join("b.json").exists());
        assert!(!root.0.join(OWNERSHIP).exists());
        assert!(!root.0.join(CONTROL).exists());
    }

    #[test]
    fn corrupted_backup_blocks_recovery_without_partial_restore() {
        let root = TestRoot::new();
        fs::write(root.0.join("a.js"), b"old").unwrap();
        let digest = file_sha256(root.0.join("a.js")).unwrap();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction
            .stage_checked("a.js", b"new", Some(&digest))
            .unwrap();
        interrupt_publication(&mut transaction, 1);
        drop(transaction);
        fs::write(root.0.join(CONTROL).join("backup/a.js"), b"corrupted").unwrap();

        let result = recover(&root.0);

        assert!(result.is_err());
        assert_eq!(fs::read(root.0.join("a.js")).unwrap(), b"new");
        assert!(root.0.join(CONTROL).join("journal.json").exists());
    }

    #[test]
    fn rejects_traversal_and_reserved_files() {
        let root = TestRoot::new();
        let mut transaction = Transaction::begin(&root.0).unwrap();

        let traversal = transaction.stage("../outside", b"bad");
        let git = transaction.stage(".git/config", b"bad");
        let metadata = transaction.stage(OWNERSHIP, b"bad");

        assert!(traversal.is_err());
        assert!(git.is_err());
        assert!(metadata.is_err());
    }

    #[test]
    fn rejects_unowned_existing_destination() {
        let root = TestRoot::new();
        fs::write(root.0.join("authored.md"), b"keep").unwrap();
        let mut transaction = Transaction::begin(&root.0).unwrap();

        let result = transaction.stage("authored.md", b"overwrite");

        assert!(result.is_err());
        assert_eq!(fs::read(root.0.join("authored.md")).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_output_escape() {
        let root = TestRoot::new();
        let outside = TestRoot::new();
        std::os::unix::fs::symlink(&outside.0, root.0.join("docs")).unwrap();
        let mut transaction = Transaction::begin(&root.0).unwrap();

        let result = transaction.stage("docs/file", b"escape");

        assert!(result.is_err());
        assert!(!outside.0.join("file").exists());
    }

    #[test]
    fn rejects_modified_candidate() {
        let root = TestRoot::new();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction.stage("app.js", b"validated").unwrap();
        fs::write(transaction.candidate_dir().join("app.js"), b"changed").unwrap();

        let result = transaction.commit();

        assert!(result.is_err());
        assert!(!root.0.join("app.js").exists());
    }

    #[test]
    fn tampered_journal_cannot_add_authored_destination() {
        let root = TestRoot::new();
        fs::write(root.0.join("authored.md"), b"keep").unwrap();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction.stage("app.js", b"new").unwrap();
        transaction.prepare().unwrap();
        let journal_path = root.0.join(CONTROL).join("journal.json");
        let mut journal: Journal = read_json(&journal_path).unwrap();
        journal.entries.insert("authored.md".to_owned(), None);
        fs::write(&journal_path, serde_json::to_vec(&journal).unwrap()).unwrap();
        drop(transaction);

        let result = recover(&root.0);

        assert!(result.is_err());
        assert_eq!(fs::read(root.0.join("authored.md")).unwrap(), b"keep");
        assert!(journal_path.exists());
    }

    #[test]
    fn completed_marker_retains_new_output_during_cleanup_recovery() {
        let root = TestRoot::new();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction.stage("app.js", b"new").unwrap();
        interrupt_publication(&mut transaction, 2);
        fs::write(root.0.join(CONTROL).join("committed"), b"complete").unwrap();
        drop(transaction);

        recover(&root.0).unwrap();

        assert_eq!(fs::read(root.0.join("app.js")).unwrap(), b"new");
        assert!(!root.0.join(CONTROL).exists());
    }

    #[test]
    fn omitted_authored_readme_is_never_deleted() {
        let root = TestRoot::new();
        let mut first = Transaction::begin(&root.0).unwrap();
        first
            .stage_authored("README.md", b"authored prose and generated section", None)
            .unwrap();
        first.commit().unwrap();
        let mut next = Transaction::begin(&root.0).unwrap();
        next.stage("app.js", b"new runtime").unwrap();

        let report = next.commit().unwrap();

        assert_eq!(report.removed, 0);
        assert_eq!(
            fs::read(root.0.join("README.md")).unwrap(),
            b"authored prose and generated section"
        );
    }

    #[test]
    fn authored_readme_updates_after_observing_user_edits() {
        let root = TestRoot::new();
        let mut first = Transaction::begin(&root.0).unwrap();
        first
            .stage_authored("README.md", b"old authored prose", None)
            .unwrap();
        first.commit().unwrap();
        fs::write(root.0.join("README.md"), b"new authored prose").unwrap();
        let digest = file_sha256(root.0.join("README.md")).unwrap();
        let mut next = Transaction::begin(&root.0).unwrap();
        next.stage_authored(
            "README.md",
            b"new authored prose plus updated section",
            Some(&digest),
        )
        .unwrap();

        next.commit().unwrap();

        assert_eq!(
            fs::read(root.0.join("README.md")).unwrap(),
            b"new authored prose plus updated section"
        );
        assert!(read_ownership(&root.0).unwrap().files.is_empty());
    }

    #[test]
    fn interrupted_authored_readme_update_restores_original() {
        let root = TestRoot::new();
        fs::write(root.0.join("README.md"), b"authored original").unwrap();
        let digest = file_sha256(root.0.join("README.md")).unwrap();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction
            .stage_authored("README.md", b"candidate", Some(&digest))
            .unwrap();
        interrupt_publication(&mut transaction, 1);
        drop(transaction);

        recover(&root.0).unwrap();

        assert_eq!(
            fs::read(root.0.join("README.md")).unwrap(),
            b"authored original"
        );
    }

    #[test]
    fn recovery_preserves_independent_edit_after_interruption() {
        let root = TestRoot::new();
        fs::write(root.0.join("README.md"), b"original authored prose").unwrap();
        let digest = file_sha256(root.0.join("README.md")).unwrap();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction
            .stage_authored("README.md", b"new generated candidate", Some(&digest))
            .unwrap();
        interrupt_publication(&mut transaction, 1);
        drop(transaction);
        fs::write(
            root.0.join("README.md"),
            b"independent user change after failure",
        )
        .unwrap();

        let result = recover(&root.0);

        assert!(result.is_err());
        assert_eq!(
            fs::read(root.0.join("README.md")).unwrap(),
            b"independent user change after failure"
        );
        assert!(root.0.join(CONTROL).join("journal.json").exists());
    }

    #[test]
    fn recovery_restores_a_partially_copied_candidate() {
        let root = TestRoot::new();
        fs::write(root.0.join("README.md"), b"old authored prose").unwrap();
        let digest = file_sha256(root.0.join("README.md")).unwrap();
        let mut transaction = Transaction::begin(&root.0).unwrap();
        transaction
            .stage_authored("README.md", b"new generated candidate", Some(&digest))
            .unwrap();
        transaction.prepare().unwrap();
        fs::write(root.0.join("README.md"), b"new generated").unwrap();
        drop(transaction);

        recover(&root.0).unwrap();

        assert_eq!(
            fs::read(root.0.join("README.md")).unwrap(),
            b"old authored prose"
        );
    }

    #[test]
    fn missing_owned_runtime_is_regenerated_after_checkout() {
        let root = TestRoot::new();
        let mut first = Transaction::begin(&root.0).unwrap();
        first.stage("docs/pkg/runtime.wasm", b"runtime").unwrap();
        first.commit().unwrap();
        fs::remove_file(root.0.join("docs/pkg/runtime.wasm")).unwrap();
        let mut next = Transaction::begin(&root.0).unwrap();

        next.stage("docs/pkg/runtime.wasm", b"runtime").unwrap();
        next.commit().unwrap();

        assert_eq!(
            fs::read(root.0.join("docs/pkg/runtime.wasm")).unwrap(),
            b"runtime"
        );
    }

    #[test]
    fn missing_stale_owned_runtime_retires_without_deletion_failure() {
        let root = TestRoot::new();
        let mut first = Transaction::begin(&root.0).unwrap();
        first.stage("docs/pkg/old.wasm", b"old runtime").unwrap();
        first.commit().unwrap();
        fs::remove_file(root.0.join("docs/pkg/old.wasm")).unwrap();
        let mut next = Transaction::begin(&root.0).unwrap();
        next.stage("docs/pkg/new.wasm", b"new runtime").unwrap();

        let report = next.commit().unwrap();

        assert_eq!(report.removed, 0);
        assert!(
            !read_ownership(&root.0)
                .unwrap()
                .files
                .contains_key("docs/pkg/old.wasm")
        );
        assert_eq!(
            fs::read(root.0.join("docs/pkg/new.wasm")).unwrap(),
            b"new runtime"
        );
    }

    #[test]
    fn concurrently_appearing_missing_owned_file_is_not_overwritten() {
        let root = TestRoot::new();
        let mut first = Transaction::begin(&root.0).unwrap();
        first.stage("runtime.js", b"old").unwrap();
        first.commit().unwrap();
        fs::remove_file(root.0.join("runtime.js")).unwrap();
        let mut next = Transaction::begin(&root.0).unwrap();
        next.stage("runtime.js", b"regenerated").unwrap();
        fs::write(root.0.join("runtime.js"), b"concurrent edit").unwrap();

        let result = next.commit();

        assert!(result.is_err());
        assert_eq!(
            fs::read(root.0.join("runtime.js")).unwrap(),
            b"concurrent edit"
        );
    }

    #[test]
    fn missing_owned_runtime_recovery_restores_absence() {
        let root = TestRoot::new();
        let mut first = Transaction::begin(&root.0).unwrap();
        first.stage("runtime.js", b"old").unwrap();
        first.commit().unwrap();
        fs::remove_file(root.0.join("runtime.js")).unwrap();
        let original_manifest = fs::read(root.0.join(OWNERSHIP)).unwrap();
        let mut next = Transaction::begin(&root.0).unwrap();
        next.stage("runtime.js", b"regenerated").unwrap();
        interrupt_publication(&mut next, 1);
        drop(next);

        recover(&root.0).unwrap();

        assert!(!root.0.join("runtime.js").exists());
        assert_eq!(fs::read(root.0.join(OWNERSHIP)).unwrap(), original_manifest);
    }

    #[test]
    fn missing_stale_output_created_concurrently_is_preserved() {
        let root = TestRoot::new();
        let mut first = Transaction::begin(&root.0).unwrap();
        first.stage("stale.js", b"old").unwrap();
        first.commit().unwrap();
        fs::remove_file(root.0.join("stale.js")).unwrap();
        let next = Transaction::begin(&root.0).unwrap();
        fs::write(root.0.join("stale.js"), b"concurrent file").unwrap();

        let result = next.commit();

        assert!(result.is_err());
        assert_eq!(
            fs::read(root.0.join("stale.js")).unwrap(),
            b"concurrent file"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn trusted_tmp_alias_resolves_to_canonical_transaction_root() {
        let name = TestRoot::new();
        let root = TestRoot(Path::new("/private/tmp").join(name.0.file_name().unwrap()));
        fs::create_dir(&root.0).unwrap();
        let alias = Path::new("/tmp").join(root.0.file_name().unwrap());

        let transaction = Transaction::begin(&alias).unwrap();

        assert!(transaction.candidate_dir().starts_with(&root.0));
    }

    #[cfg(unix)]
    #[test]
    fn custom_symlink_root_remains_rejected() {
        let root = TestRoot::new();
        let alias_parent = TestRoot::new();
        std::os::unix::fs::symlink(&root.0, alias_parent.0.join("alias")).unwrap();

        let result = Transaction::begin(alias_parent.0.join("alias"));

        assert!(result.is_err());
        assert!(!root.0.join(LOCK).exists());
    }

    #[cfg(unix)]
    #[test]
    fn custom_symlink_ancestor_diagnostic_names_rejected_component() {
        let root = TestRoot::new();
        fs::create_dir(root.0.join("child")).unwrap();
        let alias_parent = TestRoot::new();
        let alias = alias_parent.0.join("custom-alias");
        std::os::unix::fs::symlink(&root.0, &alias).unwrap();

        let result = checked_workspace_root(&alias.join("child"));

        let message = result.unwrap_err().to_string();
        assert!(message.contains(&alias.display().to_string()));
        assert!(message.contains("symlink"));
        assert!(!root.0.join(LOCK).exists());
    }

    #[test]
    fn wrong_abandoned_lock_token_is_rejected() {
        let root = TestRoot::new();
        fs::write(root.0.join(LOCK), b"known-owner").unwrap();

        let result = release_abandoned_lock(&root.0, "different-owner");

        assert!(result.is_err());
        assert!(root.0.join(LOCK).exists());
    }
}
