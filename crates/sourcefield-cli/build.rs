//! Bind the compiled CLI to the exact source inputs used by its browser bundle.

use std::{
    collections::BTreeSet,
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

/// Paths and length-delimited bytes form a portable, unambiguous source identity.
/// Keep this ordered contract aligned with scripts/runtime_manifest.py.
const INPUTS: &[&str] = &[
    "runtime/index.html",
    "runtime/app.css",
    "runtime/app.js",
    "runtime/simulation-fallback.js",
    "runtime/favicon.svg",
    "runtime/site.webmanifest",
    "crates/sourcefield-wasm/src/lib.rs",
    "crates/sourcefield-core/src/model.rs",
    "Cargo.toml",
    "Cargo.lock",
    ".cargo/config.toml",
    "rust-toolchain.toml",
    "crates/sourcefield-wasm/Cargo.toml",
    "scripts/build-wasm.sh",
    "scripts/build_wasm.py",
    "scripts/sourcefield_tools/binaryen.py",
    "tools/binaryen.json",
    "tools/sourcefield-wasm/Cargo.toml",
];

/// Fingerprint source rather than filesystem metadata so clean checkouts agree.
pub fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
        .join("../..");

    let fingerprint = runtime_fingerprint(&root);
    let revision = env::var("SOURCEFIELD_SOURCE_COMMIT").unwrap_or_else(|_| "unreleased".into());
    assert!(
        revision == "unreleased"
            || (revision.len() == 40
                && revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))),
        "SOURCEFIELD_SOURCE_COMMIT must be a lowercase full commit SHA or unreleased"
    );
    println!("cargo:rerun-if-env-changed=SOURCEFIELD_SOURCE_COMMIT");
    println!("cargo:rustc-env=SOURCEFIELD_RUNTIME_FINGERPRINT={fingerprint}");
    println!(
        "cargo:rustc-env=SOURCEFIELD_GENERATOR_FINGERPRINT={}",
        generator_fingerprint(&root)
    );
    println!("cargo:rustc-env=SOURCEFIELD_SOURCE_COMMIT={revision}");
}

/// Inventory authored Rust and package manifests, excluding build outputs and Git internals.
fn collect_generator_sources(root: &Path, directory: &Path, names: &mut BTreeSet<String>) {
    let metadata = fs::symlink_metadata(directory).expect("inspect source directory");
    assert!(
        metadata.is_dir(),
        "source directory must be a real directory"
    );

    for entry in fs::read_dir(directory).expect("read generator source directory") {
        let entry = entry.expect("read generator source entry");
        let path = entry.path();
        let kind = entry.file_type().expect("inspect generator source entry");
        assert!(
            !kind.is_symlink(),
            "symlink in generator source: {}",
            path.display()
        );

        if kind.is_dir() {
            if !matches!(entry.file_name().to_str(), Some("target" | ".git")) {
                collect_generator_sources(root, &path, names);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs")
            || entry.file_name() == "Cargo.toml"
        {
            assert!(
                kind.is_file(),
                "generator source must be a regular file: {}",
                path.display()
            );
            // Slash-separated relative names make native fingerprints agree across host platforms.
            let name = path
                .strip_prefix(root)
                .expect("source beneath workspace")
                .components()
                .map(|component| component.as_os_str().to_str().expect("UTF-8 source path"))
                .collect::<Vec<_>>()
                .join("/");

            assert!(name.is_ascii(), "source input paths must be ASCII");
            names.insert(name);
        }
    }
}

/// Bind local replay to the entire generator, not only browser-compatible schema changes.
/// An unreleased native implementation edit must invalidate an older captured generation.
pub fn generator_fingerprint(root: &Path) -> String {
    let mut names = runtime_inputs(root).into_iter().collect::<BTreeSet<_>>();
    collect_generator_sources(root, &root.join("crates"), &mut names);
    // Watching the directory also detects newly added or removed Rust sources and manifests.
    println!("cargo:rerun-if-changed={}", root.join("crates").display());
    let mut digest = Sha256::new();
    digest.update(b"sourcefield-generator-source-v1\0");

    for name in names {
        let path = root.join(&name);
        let bytes = read_regular(root, &name);
        println!("cargo:rerun-if-changed={}", path.display());
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }

    hex::encode(digest.finalize())
}

/// Discover future host-tool modules in the same sorted order as runtime_manifest.py.
fn runtime_inputs(root: &Path) -> Vec<String> {
    let mut helper_sources = BTreeSet::new();
    let directory = root.join("tools/sourcefield-wasm");
    collect_generator_sources(root, &directory, &mut helper_sources);
    helper_sources.retain(|name| !INPUTS.contains(&name.as_str()));
    assert!(
        helper_sources.contains("tools/sourcefield-wasm/src/main.rs"),
        "binding helper main.rs is required"
    );
    println!("cargo:rerun-if-changed={}", directory.display());

    INPUTS
        .iter()
        .map(|name| (*name).to_owned())
        .chain(helper_sources)
        .collect()
}

/// Reject substituted source components and bound allocations just like the Python manifest reader.
fn read_regular(root: &Path, relative: &str) -> Vec<u8> {
    const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
    let mut path = root.to_path_buf();

    for component in Path::new(relative).components() {
        path.push(component);
        let metadata = fs::symlink_metadata(&path)
            .unwrap_or_else(|error| panic!("inspect {relative}: {error}"));

        assert!(
            !metadata.is_symlink(),
            "symlink in source input: {relative}"
        );
    }

    let metadata =
        fs::metadata(&path).unwrap_or_else(|error| panic!("inspect {relative}: {error}"));

    assert!(
        metadata.is_file() && metadata.len() <= MAX_FILE_BYTES,
        "source input must be a regular file within 16 MiB: {relative}"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(&path)
        .unwrap_or_else(|error| panic!("open {relative}: {error}"))
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .unwrap_or_else(|error| panic!("read {relative}: {error}"));
    assert!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "source input grew beyond 16 MiB: {relative}"
    );

    bytes
}

/// Preserve the independently versioned browser/native source agreement contract.
pub fn runtime_fingerprint(root: &Path) -> String {
    let mut digest = Sha256::new();
    digest.update(b"sourcefield-runtime-source-v1\0");

    for name in runtime_inputs(root) {
        let path = root.join(&name);
        let bytes = read_regular(root, &name);
        println!("cargo:rerun-if-changed={}", path.display());
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }

    hex::encode(digest.finalize())
}
