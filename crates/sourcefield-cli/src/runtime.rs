//! Assemble browser assets only after verifying agreement with the compiled generator.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sourcefield_io::{read_file_bounded, sha256_bytes};
use sourcefield_workspace::Transaction;

/// Authored runtime files embedded from the same checkout as the native generator.
const FILES: &[(&str, &[u8])] = &[
    ("index.html", include_bytes!("../../../runtime/index.html")),
    ("app.css", include_bytes!("../../../runtime/app.css")),
    ("app.js", include_bytes!("../../../runtime/app.js")),
    (
        "simulation-fallback.js",
        include_bytes!("../../../runtime/simulation-fallback.js"),
    ),
    (
        "favicon.svg",
        include_bytes!("../../../runtime/favicon.svg"),
    ),
    (
        "site.webmanifest",
        include_bytes!("../../../runtime/site.webmanifest"),
    ),
];
const WASM_FILES: &[&str] = &["pkg/sourcefield_wasm.js", "pkg/sourcefield_wasm_bg.wasm"];
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Local agreement metadata; authenticity is established by external release attestations.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeManifest {
    /// Manifest contract version, independent of generator SemVer.
    schema_version: u32,
    /// Native/browser release version that must match the executing CLI.
    generator_version: String,
    /// Full released commit or the explicit local marker `unreleased`.
    source_revision: String,
    /// Digest of the ordered, length-delimited source inputs used by build.rs.
    source_fingerprint: String,
    /// SHA-256 digests keyed by the exact allowed bundle-relative file paths.
    files: BTreeMap<String, String>,
}

/// Stage one complete runtime after validating every external file before any staging.
pub(crate) fn stage(
    transaction: &mut Transaction,
    root: &Path,
    docs: &Path,
    runtime: Option<&Path>,
    adopt: bool,
) -> Result<()> {
    if let Some(directory) = runtime {
        let bundle = verified_bundle(directory)?;

        for (name, bytes) in bundle {
            super::stage_generated(transaction, root, &docs.join(name), &bytes, adopt)?;
        }
    } else {
        for (name, embedded) in FILES {
            super::stage_generated(transaction, root, &docs.join(name), embedded, adopt)?;
        }
    }

    super::stage_generated(transaction, root, &docs.join(".nojekyll"), b"", adopt)?;

    Ok(())
}

/// Validate a complete bundle against compile-time identity and retain exactly verified bytes.
/// Reading once prevents a file changed after validation from being reread during staging.
fn verified_bundle(directory: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let metadata = fs::symlink_metadata(directory).context("read runtime directory")?;

    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "runtime root must be a regular directory"
    );

    let manifest_bytes = bounded_read(directory, "runtime-manifest.json")
        .context("read runtime manifest; rebuild the matching bundle with scripts/build-wasm.sh")?;

    let manifest: RuntimeManifest =
        serde_json::from_slice(&manifest_bytes).context("parse runtime manifest")?;

    ensure!(
        manifest.schema_version == 1,
        "unsupported runtime manifest schema"
    );
    ensure!(
        manifest.generator_version == env!("CARGO_PKG_VERSION"),
        "runtime generator version differs from compiled CLI"
    );
    ensure!(
        manifest.source_fingerprint == env!("SOURCEFIELD_RUNTIME_FINGERPRINT"),
        "runtime source fingerprint differs from compiled CLI; rebuild CLI and runtime together"
    );
    ensure!(
        manifest.source_revision == env!("SOURCEFIELD_SOURCE_COMMIT"),
        "runtime source revision differs from compiled CLI"
    );
    ensure!(
        manifest.files.len() == FILES.len() + WASM_FILES.len(),
        "runtime manifest must describe exactly the complete bundle"
    );

    // The exact allowlist rejects absolute paths, traversal, platform separators, and extra files.
    for name in manifest.files.keys() {
        ensure!(
            FILES.iter().any(|(allowed, _)| name == allowed) || WASM_FILES.contains(&name.as_str()),
            "unexpected runtime manifest path: {name}"
        );
    }

    let mut bundle = Vec::with_capacity(manifest.files.len() + 1);

    for (name, expected_digest) in &manifest.files {
        let bytes = bounded_read(directory, name)?;
        let actual_digest = sha256_bytes(&bytes);

        ensure!(
            &actual_digest == expected_digest,
            "runtime digest mismatch: {name}"
        );

        if let Some((_, embedded)) = FILES.iter().find(|(allowed, _)| name == allowed) {
            ensure!(
                bytes == *embedded,
                "runtime authored asset differs from compiled CLI: {name}"
            );
        }

        if name.ends_with(".wasm") {
            ensure!(
                bytes.starts_with(b"\0asm\x01\0\0\0"),
                "invalid WebAssembly v1 module header"
            );
        }

        bundle.push((name.clone(), bytes));
    }

    bundle.push(("runtime-manifest.json".into(), manifest_bytes));

    Ok(bundle)
}

/// Bound reads even if a file grows, and reject symlinks in bundle-relative components.
fn bounded_read(directory: &Path, relative: &str) -> Result<Vec<u8>> {
    let mut path = directory.to_path_buf();

    for component in Path::new(relative).components() {
        ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "invalid runtime path"
        );
        path.push(component);
        ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "runtime entry must not be a symlink"
        );
    }

    let file = fs::File::open(&path).with_context(|| format!("read runtime {}", path.display()))?;
    let metadata = file.metadata()?;

    ensure!(metadata.is_file(), "runtime entry must be a regular file");
    ensure!(
        metadata.len() <= MAX_FILE_BYTES,
        "runtime file exceeds 16 MiB limit"
    );

    read_file_bounded(file, MAX_FILE_BYTES).context("runtime file grew beyond 16 MiB limit")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// Isolate each bundle fixture without introducing a test-only dependency.
    struct Fixture(std::path::PathBuf);

    impl Fixture {
        /// Create a complete matching synthetic bundle for one isolated test.
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "sourcefield-runtime-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));

            fs::create_dir_all(root.join("pkg")).unwrap();
            let mut files = BTreeMap::new();

            for (name, bytes) in FILES {
                fs::write(root.join(name), bytes).unwrap();
                files.insert((*name).into(), sha256_bytes(bytes));
            }

            for (name, bytes) in [
                (WASM_FILES[0], b"export default function() {}".as_slice()),
                (WASM_FILES[1], b"\0asm\x01\0\0\0".as_slice()),
            ] {
                fs::write(root.join(name), bytes).unwrap();
                files.insert(name.into(), sha256_bytes(bytes));
            }

            let manifest = RuntimeManifest {
                schema_version: 1,
                generator_version: env!("CARGO_PKG_VERSION").into(),
                source_revision: env!("SOURCEFIELD_SOURCE_COMMIT").into(),
                source_fingerprint: env!("SOURCEFIELD_RUNTIME_FINGERPRINT").into(),
                files,
            };

            let fixture = Self(root);
            fixture.write_manifest(&manifest);

            fixture
        }

        /// Read mutable test metadata without touching compiled source inputs.
        fn manifest(&self) -> RuntimeManifest {
            serde_json::from_slice(&fs::read(self.0.join("runtime-manifest.json")).unwrap())
                .unwrap()
        }

        /// Replace fixture metadata before invoking the validation boundary.
        fn write_manifest(&self, manifest: &RuntimeManifest) {
            fs::write(
                self.0.join("runtime-manifest.json"),
                serde_json::to_vec(manifest).unwrap(),
            )
            .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn complete_matching_bundle_is_accepted() {
        let fixture = Fixture::new();

        let bundle = verified_bundle(&fixture.0).unwrap();

        assert_eq!(bundle.len(), 9);
    }

    #[test]
    fn replaced_wasm_fails_before_staging() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join(WASM_FILES[1]), b"\0asm\x01\0\0\0different").unwrap();

        let result = verified_bundle(&fixture.0);

        assert!(result.unwrap_err().to_string().contains("digest mismatch"));
    }

    #[test]
    fn stale_source_fingerprint_is_rejected() {
        let fixture = Fixture::new();
        let mut manifest = fixture.manifest();
        manifest.source_fingerprint = "0".repeat(64);
        fixture.write_manifest(&manifest);

        let result = verified_bundle(&fixture.0);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("fingerprint differs")
        );
    }

    #[test]
    fn different_generator_version_is_rejected() {
        let fixture = Fixture::new();
        let mut manifest = fixture.manifest();
        manifest.generator_version = "999.0.0".into();
        fixture.write_manifest(&manifest);

        let result = verified_bundle(&fixture.0);

        assert!(result.unwrap_err().to_string().contains("version differs"));
    }

    #[test]
    fn different_release_revision_is_rejected() {
        let fixture = Fixture::new();
        let mut manifest = fixture.manifest();
        manifest.source_revision = if env!("SOURCEFIELD_SOURCE_COMMIT") == "unreleased" {
            "a".repeat(40)
        } else {
            "unreleased".into()
        };

        fixture.write_manifest(&manifest);

        let result = verified_bundle(&fixture.0);

        assert!(result.unwrap_err().to_string().contains("revision differs"));
    }

    #[test]
    fn traversal_manifest_path_is_rejected() {
        let fixture = Fixture::new();
        let mut manifest = fixture.manifest();
        let digest = manifest.files.remove("app.js").unwrap();
        manifest.files.insert("../app.js".into(), digest);
        fixture.write_manifest(&manifest);

        let result = verified_bundle(&fixture.0);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unexpected runtime manifest path")
        );
    }

    #[test]
    fn updated_digest_cannot_replace_embedded_authored_runtime() {
        let fixture = Fixture::new();
        let mut manifest = fixture.manifest();
        let bytes = b"different browser implementation";
        fs::write(fixture.0.join("app.js"), bytes).unwrap();
        manifest.files.insert("app.js".into(), sha256_bytes(bytes));
        fixture.write_manifest(&manifest);

        let result = verified_bundle(&fixture.0);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("authored asset differs")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_wasm_directory_is_rejected() {
        let fixture = Fixture::new();
        fs::rename(fixture.0.join("pkg"), fixture.0.join("actual-pkg")).unwrap();
        std::os::unix::fs::symlink("actual-pkg", fixture.0.join("pkg")).unwrap();

        let result = verified_bundle(&fixture.0);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must not be a symlink")
        );
    }
}
