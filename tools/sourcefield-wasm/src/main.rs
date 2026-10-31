//! Generate Sourcefield web bindings from a local Cargo-produced WebAssembly module.

use std::{env, ffi::OsString, fs, path::PathBuf, process::ExitCode};

use anyhow::{Context, Result, ensure};
use wasm_bindgen_cli_support::Bindgen;

/// Select exactly one input module and an existing staging directory.
fn arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<(PathBuf, PathBuf)> {
    let mut arguments = arguments.into_iter();
    let input = arguments
        .next()
        .context("usage: sourcefield-wasm-build INPUT.wasm OUTPUT_DIRECTORY")?;

    let output = arguments
        .next()
        .context("usage: sourcefield-wasm-build INPUT.wasm OUTPUT_DIRECTORY")?;

    ensure!(
        arguments.next().is_none(),
        "expected exactly two path arguments"
    );

    Ok((input.into(), output.into()))
}

/// Preserve the official web CLI defaults without its network and test-server dependencies.
fn generate(input: PathBuf, output: PathBuf) -> Result<()> {
    let metadata = fs::symlink_metadata(&input).context("read local WebAssembly input metadata")?;
    ensure!(
        metadata.is_file(),
        "WebAssembly input must be a regular file"
    );
    let metadata =
        fs::symlink_metadata(&output).context("read binding staging directory metadata")?;

    ensure!(
        metadata.is_dir(),
        "binding output must be an existing directory"
    );

    // The support library defaults differ from the CLI: retain typings and import.meta.url loading.
    Bindgen::new()
        .input_path(&input)
        .out_name("sourcefield_wasm")
        .typescript(true)
        .omit_default_module_path(false)
        .web(true)?
        .generate(&output)
        .context("generate Sourcefield web bindings")?;

    Ok(())
}

/// Report bounded context rather than serializing tool input or generated output.
fn main() -> ExitCode {
    let result =
        arguments(env::args_os().skip(1)).and_then(|(input, output)| generate(input, output));

    if let Err(error) = result {
        eprintln!("Sourcefield web binding generation failed: {error:#}");

        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// Isolate local input/output fixtures without adding a runtime dependency.
    struct Fixture(PathBuf);

    impl Fixture {
        /// Use distinct directories for independently scheduled tests.
        fn new() -> Self {
            let root = env::temp_dir().join(format!(
                "sourcefield-bindings-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));

            fs::create_dir_all(root.join("output")).unwrap();

            Self(root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // Cleanup must not hide an earlier assertion by panicking during unwinding.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_output_argument_is_rejected() {
        // Arrange
        let input = [OsString::from("input.wasm")];

        // Act
        let result = arguments(input);

        // Assert
        assert!(result.unwrap_err().to_string().contains("usage:"));
    }

    #[test]
    fn extra_argument_is_rejected() {
        // Arrange
        let input = ["input.wasm", "output", "extra"].map(OsString::from);

        // Act
        let result = arguments(input);

        // Assert
        assert!(result.unwrap_err().to_string().contains("exactly two"));
    }

    #[test]
    fn missing_input_is_rejected_before_writing_output() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.0.join("output");

        // Act
        let result = generate(fixture.0.join("missing.wasm"), output.clone());

        // Assert
        assert!(result.unwrap_err().to_string().contains("input metadata"));
        assert_eq!(fs::read_dir(output).unwrap().count(), 0);
    }

    #[test]
    fn invalid_wasm_is_rejected() {
        // Arrange
        let fixture = Fixture::new();
        let input = fixture.0.join("input.wasm");
        fs::write(&input, b"invalid WebAssembly").unwrap();

        // Act
        let result = generate(input, fixture.0.join("output"));

        // Assert
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("generate Sourcefield")
        );
    }

    #[test]
    fn file_output_is_rejected_without_replacement() {
        // Arrange
        let fixture = Fixture::new();
        let input = fixture.0.join("input.wasm");
        let output = fixture.0.join("output.txt");
        fs::write(&input, b"\0asm\x01\0\0\0").unwrap();
        fs::write(&output, b"preserve me").unwrap();

        // Act
        let result = generate(input, output.clone());

        // Assert
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("existing directory")
        );
        assert_eq!(fs::read(output).unwrap(), b"preserve me");
    }

    #[test]
    fn valid_module_keeps_web_defaults_and_typings() {
        // Arrange
        let fixture = Fixture::new();
        let input = fixture.0.join("input.wasm");
        let output = fixture.0.join("output");
        fs::write(&input, b"\0asm\x01\0\0\0").unwrap();

        // Act
        let result = generate(input, output.clone());

        // Assert
        assert!(result.is_ok(), "web binding generation should succeed");
        let javascript = fs::read_to_string(output.join("sourcefield_wasm.js")).unwrap();
        assert!(javascript.contains("new URL('sourcefield_wasm_bg.wasm', import.meta.url)"));
        assert!(javascript.contains("__wbg_init as default"));
        assert!(output.join("sourcefield_wasm.d.ts").is_file());
        assert!(output.join("sourcefield_wasm_bg.wasm.d.ts").is_file());
        assert!(output.join("sourcefield_wasm_bg.wasm").is_file());
    }
}
