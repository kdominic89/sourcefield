//! Exercise the actual build-script fingerprint algorithms on isolated source trees.

#[path = "../build.rs"]
pub mod build_script;

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Synthetic authored inputs avoid mutating the shared checkout during parallel tests.
struct Fixture(PathBuf);

impl Fixture {
    /// Create both browser-contract inputs and independent native implementation files.
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sourcefield-fingerprint-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        for name in [
            "runtime/index.html",
            "runtime/app.css",
            "runtime/app.js",
            "runtime/simulation-fallback.js",
            "runtime/favicon.svg",
            "runtime/site.webmanifest",
            "crates/sourcefield-wasm/src/lib.rs",
            "crates/sourcefield-core/src/model.rs",
            "Cargo.lock",
            "rust-toolchain.toml",
            "Cargo.toml",
            ".cargo/config.toml",
            "crates/sourcefield-wasm/Cargo.toml",
            "scripts/build-wasm.sh",
            "scripts/build_wasm.py",
            "scripts/sourcefield_tools/binaryen.py",
            "tools/binaryen.json",
            "tools/sourcefield-wasm/Cargo.toml",
            "tools/sourcefield-wasm/src/main.rs",
            "crates/sourcefield-cli/build.rs",
            "crates/sourcefield-cli/Cargo.toml",
            "crates/sourcefield-cli/src/main.rs",
            "crates/sourcefield-render/src/lib.rs",
        ] {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, format!("fixture: {name}\n")).unwrap();
        }

        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A cleanup error must not replace the assertion that caused unwinding.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_edit_invalidates_generator_without_changing_runtime_contract() {
    let fixture = Fixture::new();
    let generator_before = build_script::generator_fingerprint(&fixture.0);
    let runtime_before = build_script::runtime_fingerprint(&fixture.0);

    fs::write(
        fixture.0.join("crates/sourcefield-render/src/lib.rs"),
        "changed renderer",
    )
    .unwrap();
    let generator_after = build_script::generator_fingerprint(&fixture.0);
    let runtime_after = build_script::runtime_fingerprint(&fixture.0);

    assert_ne!(generator_before, generator_after);
    assert_eq!(runtime_before, runtime_after);
}

#[test]
fn identical_sources_agree_across_checkout_locations() {
    let left = Fixture::new();
    let right = Fixture::new();

    let left_generator = build_script::generator_fingerprint(&left.0);
    let right_generator = build_script::generator_fingerprint(&right.0);

    assert_eq!(left_generator, right_generator);
}

#[test]
fn generated_build_files_do_not_affect_source_identity() {
    let fixture = Fixture::new();
    let before = build_script::generator_fingerprint(&fixture.0);

    for name in [
        "crates/sourcefield-cli/target/generated.rs",
        "crates/.git/ignored.rs",
    ] {
        let path = fixture.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "generated noise").unwrap();
    }

    let after = build_script::generator_fingerprint(&fixture.0);

    assert_eq!(before, after);
}

#[test]
fn added_rust_module_changes_source_identity() {
    let fixture = Fixture::new();
    let before = build_script::generator_fingerprint(&fixture.0);

    fs::write(
        fixture.0.join("crates/sourcefield-render/src/extra.rs"),
        "new module",
    )
    .unwrap();
    let after = build_script::generator_fingerprint(&fixture.0);

    assert_ne!(before, after);
}

#[test]
fn removed_rust_module_changes_source_identity() {
    let fixture = Fixture::new();
    let before = build_script::generator_fingerprint(&fixture.0);

    fs::remove_file(fixture.0.join("crates/sourcefield-render/src/lib.rs")).unwrap();
    let after = build_script::generator_fingerprint(&fixture.0);

    assert_ne!(before, after);
}

#[test]
fn crate_manifest_edit_changes_source_identity() {
    let fixture = Fixture::new();
    let before = build_script::generator_fingerprint(&fixture.0);

    fs::write(
        fixture.0.join("crates/sourcefield-cli/Cargo.toml"),
        "changed features",
    )
    .unwrap();
    let after = build_script::generator_fingerprint(&fixture.0);

    assert_ne!(before, after);
}

#[test]
fn build_script_edit_changes_source_identity() {
    let fixture = Fixture::new();
    let before = build_script::generator_fingerprint(&fixture.0);

    fs::write(
        fixture.0.join("crates/sourcefield-cli/build.rs"),
        "changed compile binding",
    )
    .unwrap();
    let after = build_script::generator_fingerprint(&fixture.0);

    assert_ne!(before, after);
}

#[test]
fn missing_required_source_fails_instead_of_publishing_partial_identity() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("Cargo.lock")).unwrap();

    let result = std::panic::catch_unwind(|| build_script::generator_fingerprint(&fixture.0));

    assert!(result.is_err());
}

#[test]
fn optimizer_manifest_changes_runtime_and_generator_identity() {
    let fixture = Fixture::new();
    let generator_before = build_script::generator_fingerprint(&fixture.0);
    let runtime_before = build_script::runtime_fingerprint(&fixture.0);

    fs::write(
        fixture.0.join("tools/binaryen.json"),
        "changed optimizer digest",
    )
    .unwrap();
    let generator_after = build_script::generator_fingerprint(&fixture.0);
    let runtime_after = build_script::runtime_fingerprint(&fixture.0);

    assert_ne!(generator_before, generator_after);
    assert_ne!(runtime_before, runtime_after);
}

#[test]
fn missing_optimizer_manifest_fails_instead_of_publishing_partial_runtime_identity() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("tools/binaryen.json")).unwrap();

    let result = std::panic::catch_unwind(|| build_script::runtime_fingerprint(&fixture.0));

    assert!(result.is_err());
}

#[test]
fn added_binding_module_changes_runtime_and_generator_identity() {
    // Arrange
    let fixture = Fixture::new();
    let generator_before = build_script::generator_fingerprint(&fixture.0);
    let runtime_before = build_script::runtime_fingerprint(&fixture.0);

    // Act
    fs::write(
        fixture.0.join("tools/sourcefield-wasm/src/extra.rs"),
        "new adapter module",
    )
    .unwrap();
    let generator_after = build_script::generator_fingerprint(&fixture.0);
    let runtime_after = build_script::runtime_fingerprint(&fixture.0);

    // Assert
    assert_ne!(generator_before, generator_after);
    assert_ne!(runtime_before, runtime_after);
}

#[test]
fn removed_binding_module_changes_runtime_and_generator_identity() {
    // Arrange
    let fixture = Fixture::new();
    let module = fixture.0.join("tools/sourcefield-wasm/src/extra.rs");
    fs::write(&module, "adapter module").unwrap();
    let generator_before = build_script::generator_fingerprint(&fixture.0);
    let runtime_before = build_script::runtime_fingerprint(&fixture.0);

    // Act
    fs::remove_file(module).unwrap();
    let generator_after = build_script::generator_fingerprint(&fixture.0);
    let runtime_after = build_script::runtime_fingerprint(&fixture.0);

    // Assert
    assert_ne!(generator_before, generator_after);
    assert_ne!(runtime_before, runtime_after);
}

#[test]
fn missing_binding_entrypoint_rejects_partial_runtime_identity() {
    // Arrange
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("tools/sourcefield-wasm/src/main.rs")).unwrap();

    // Act
    let result = std::panic::catch_unwind(|| build_script::runtime_fingerprint(&fixture.0));

    // Assert
    assert!(result.is_err());
}

#[test]
fn binding_build_outputs_do_not_change_runtime_identity() {
    // Arrange
    let fixture = Fixture::new();
    let before = build_script::runtime_fingerprint(&fixture.0);
    let output = fixture
        .0
        .join("tools/sourcefield-wasm/src/target/generated.rs");
    fs::create_dir_all(output.parent().unwrap()).unwrap();

    // Act
    fs::write(output, "compiler output").unwrap();
    let after = build_script::runtime_fingerprint(&fixture.0);

    // Assert
    assert_eq!(before, after);
}

#[cfg(unix)]
#[test]
fn symlinked_binding_module_rejects_substituted_runtime_identity() {
    // Arrange
    let fixture = Fixture::new();
    let source = fixture.0.join("tools/sourcefield-wasm/src/main.rs");
    let original = fixture.0.join("original.rs");
    fs::rename(&source, &original).unwrap();
    std::os::unix::fs::symlink(&original, &source).unwrap();

    // Act
    let result = std::panic::catch_unwind(|| build_script::runtime_fingerprint(&fixture.0));

    // Assert
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_build_driver_rejects_substituted_runtime_identity() {
    // Arrange
    let fixture = Fixture::new();
    let source = fixture.0.join("scripts/build_wasm.py");
    let original = fixture.0.join("original.py");
    fs::rename(&source, &original).unwrap();
    std::os::unix::fs::symlink(&original, &source).unwrap();

    // Act
    let result = std::panic::catch_unwind(|| build_script::runtime_fingerprint(&fixture.0));

    // Assert
    assert!(result.is_err());
}

#[test]
fn added_binding_build_script_changes_runtime_and_generator_identity() {
    // Arrange
    let fixture = Fixture::new();
    let generator_before = build_script::generator_fingerprint(&fixture.0);
    let runtime_before = build_script::runtime_fingerprint(&fixture.0);

    // Act
    fs::write(
        fixture.0.join("tools/sourcefield-wasm/build.rs"),
        "fn main() {}",
    )
    .unwrap();
    let generator_after = build_script::generator_fingerprint(&fixture.0);
    let runtime_after = build_script::runtime_fingerprint(&fixture.0);

    // Assert
    assert_ne!(generator_before, generator_after);
    assert_ne!(runtime_before, runtime_after);
}
