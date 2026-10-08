//! Assemble browser assets only after verifying agreement with the compiled generator.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sourcefield_core::{ProfileVariant, StateProfile};
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
    profile: &StateProfile,
) -> Result<()> {
    let mut bundle = if let Some(directory) = runtime {
        verified_bundle(directory)?
    } else {
        FILES
            .iter()
            .map(|(name, bytes)| ((*name).into(), bytes.to_vec()))
            .collect()
    };

    // Authenticate the complete unmodified release first. Consumer identity is generated output,
    // so its digests describe the projection without changing the verified source fingerprint.
    project_bundle(&mut bundle, profile)?;

    for (name, bytes) in bundle {
        super::stage_generated(transaction, root, &docs.join(name), &bytes, adopt)?;
    }

    super::stage_generated(transaction, root, &docs.join(".nojekyll"), b"", adopt)?;

    Ok(())
}

/// Project validated profile identity into generated browser metadata after bundle verification.
fn project_bundle(bundle: &mut [(String, Vec<u8>)], profile: &StateProfile) -> Result<()> {
    let organization = profile.variant == ProfileVariant::Organization;
    let handle = if organization {
        &profile.organization
    } else {
        &profile.username
    };

    let description = if organization {
        format!("{handle}: {}", profile.headline)
    } else {
        format!("SOURCEFIELD: {}", profile.headline)
    };

    let variant = if organization {
        "organization"
    } else {
        "personal"
    };

    for (name, bytes) in bundle.iter_mut() {
        match name.as_str() {
            "index.html" => {
                let mut html = String::from_utf8(std::mem::take(bytes))?;
                html = replace_once(
                    html,
                    "<html lang=\"en\">",
                    &format!("<html lang=\"en\" data-profile-variant=\"{variant}\">"),
                )?;
                html = replace_once(
                    html,
                    "<title>SOURCEFIELD</title>",
                    &format!(
                        "<title>SOURCEFIELD / {}</title>",
                        sourcefield_render::escape_xml(handle)
                    ),
                )?;
                html = replace_once(
                    html,
                    "content=\"An interactive field of projects, packages, and capabilities.\"",
                    &format!(
                        "content=\"{}\"",
                        sourcefield_render::escape_xml(&description)
                    ),
                )?;
                html = replace_once(
                    html,
                    "<a class=\"identity\" href=\"#\"",
                    &format!(
                        "<a class=\"identity\" href=\"https://github.com/{}\"",
                        sourcefield_render::escape_xml(handle)
                    ),
                )?;
                html = replace_once(
                    html,
                    "<small>/ PROFILE</small>",
                    &format!(
                        "<small>/ {}</small>",
                        sourcefield_render::escape_xml(handle)
                    ),
                )?;
                *bytes = html.into_bytes();
            }
            "site.webmanifest" => {
                let mut pwa: serde_json::Value = serde_json::from_slice(bytes)?;
                pwa["name"] = format!("SOURCEFIELD / {handle}").into();
                pwa["short_name"] = if organization {
                    handle.as_str()
                } else {
                    "SOURCEFIELD"
                }
                .into();
                pwa["description"] = description.clone().into();
                *bytes = serde_json::to_vec_pretty(&pwa)?;
                bytes.push(b'\n');
            }
            "favicon.svg" if organization => {
                let mut svg = String::from_utf8(std::mem::take(bytes))?;
                svg = replace_once(svg, "#A78BFA", "#FFB86B")?;
                svg = replace_once(
                    svg,
                    "<circle cx=\"32\" cy=\"32\" r=\"6\" fill=\"url(#g)\"/>",
                    concat!(
                        "<path d=\"M32 22L41 27V37L32 42L23 37V27Z\" fill=\"none\" ",
                        "stroke=\"url(#g)\" stroke-width=\"2\"/>"
                    ),
                )?;
                *bytes = svg.into_bytes();
            }
            _ => {}
        }
    }

    // Release manifests authenticate source bytes; deployed manifests retain that source identity
    // and report the actual generated file bytes. Neither manifest authenticates a release alone.
    if let Some(index) = bundle
        .iter()
        .position(|(name, _)| name == "runtime-manifest.json")
    {
        let mut manifest: RuntimeManifest = serde_json::from_slice(&bundle[index].1)?;

        for (name, bytes) in bundle.iter() {
            // WASM and unchanged assets already have verified digests.
            // Reuse them instead of hashing large modules twice.
            if matches!(
                name.as_str(),
                "index.html" | "site.webmanifest" | "favicon.svg"
            ) {
                *manifest
                    .files
                    .get_mut(name)
                    .context("missing projected runtime member")? = sha256_bytes(bytes);
            }
        }

        bundle[index].1 = serde_json::to_vec_pretty(&manifest)?;
        bundle[index].1.push(b'\n');
    }

    Ok(())
}

/// Fail visibly when a compiled template changes instead of silently losing generated identity.
fn replace_once(source: String, marker: &str, replacement: &str) -> Result<String> {
    ensure!(
        source.matches(marker).count() == 1,
        "runtime template marker is missing or repeated"
    );

    Ok(source.replacen(marker, replacement, 1))
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
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn complete_matching_bundle_is_accepted() {
        let fixture = Fixture::new();

        let bundle = verified_bundle(&fixture.0).unwrap();

        assert_eq!(bundle.len(), 9);
    }

    /// Build public identity without coupling projection tests to a profile owner's name.
    fn profile(variant: ProfileVariant) -> StateProfile {
        StateProfile {
            variant,
            maintainer: None,
            username: "sample-person".into(),
            display_name: "Sample Person".into(),
            organization: "example-labs".into(),
            headline: "Libraries and developer tools.".into(),
            tagline: "Sample profile.".into(),
            pages_url: "https://example.github.io/profile/".into(),
            source_url: "https://github.com/example/profile".into(),
        }
    }

    /// Borrow a projected fixture member to inspect its rendered contract.
    fn member<'a>(bundle: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
        &bundle.iter().find(|(key, _)| key == name).unwrap().1
    }

    #[test]
    fn organization_projection_restores_identity_and_hexagonal_favicon() {
        // Arrange
        let fixture = Fixture::new();
        let mut bundle = verified_bundle(&fixture.0).unwrap();
        let profile = profile(ProfileVariant::Organization);

        // Act
        project_bundle(&mut bundle, &profile).unwrap();
        let html = std::str::from_utf8(member(&bundle, "index.html")).unwrap();
        let favicon = std::str::from_utf8(member(&bundle, "favicon.svg")).unwrap();
        let pwa: serde_json::Value =
            serde_json::from_slice(member(&bundle, "site.webmanifest")).unwrap();

        // Assert
        assert!(html.contains("<title>SOURCEFIELD / example-labs</title>"));
        assert!(html.contains("data-profile-variant=\"organization\""));
        assert!(html.contains("content=\"example-labs: Libraries and developer tools.\""));
        assert!(html.contains("href=\"https://github.com/example-labs\""));
        assert!(favicon.contains("M32 22L41 27V37L32 42L23 37V27Z"));
        assert!(favicon.contains("stop-color=\"#FFB86B\""));
        assert!(!favicon.contains("r=\"6\""));
        assert_eq!(pwa["name"], "SOURCEFIELD / example-labs");
        assert_eq!(pwa["short_name"], "example-labs");
        assert_eq!(
            pwa["description"],
            "example-labs: Libraries and developer tools."
        );
    }

    #[test]
    fn personal_projection_restores_identity_without_changing_its_favicon() {
        // Arrange
        let fixture = Fixture::new();
        let mut bundle = verified_bundle(&fixture.0).unwrap();
        let profile = profile(ProfileVariant::Personal);
        let original_favicon = member(&bundle, "favicon.svg").to_vec();

        // Act
        project_bundle(&mut bundle, &profile).unwrap();
        let html = std::str::from_utf8(member(&bundle, "index.html")).unwrap();
        let pwa: serde_json::Value =
            serde_json::from_slice(member(&bundle, "site.webmanifest")).unwrap();

        // Assert
        assert!(html.contains("<title>SOURCEFIELD / sample-person</title>"));
        assert!(html.contains("data-profile-variant=\"personal\""));
        assert!(html.contains("content=\"SOURCEFIELD: Libraries and developer tools.\""));
        assert_eq!(member(&bundle, "favicon.svg"), original_favicon);
        assert_eq!(pwa["name"], "SOURCEFIELD / sample-person");
        assert_eq!(pwa["short_name"], "SOURCEFIELD");
    }

    #[test]
    fn authored_metadata_is_escaped_as_markup_and_serialized_as_json() {
        // Arrange
        let fixture = Fixture::new();
        let mut bundle = verified_bundle(&fixture.0).unwrap();
        let mut profile = profile(ProfileVariant::Organization);
        profile.headline = "<script> & \"quoted\" 'text'".into();

        // Act
        project_bundle(&mut bundle, &profile).unwrap();
        let html = std::str::from_utf8(member(&bundle, "index.html")).unwrap();
        let pwa: serde_json::Value =
            serde_json::from_slice(member(&bundle, "site.webmanifest")).unwrap();

        // Assert
        assert!(html.contains("&lt;script&gt; &amp; &quot;quoted&quot; &apos;text&apos;"));
        assert!(!html.contains("<script>"));
        assert_eq!(
            pwa["description"],
            format!("example-labs: {}", profile.headline)
        );
    }

    #[test]
    fn projected_manifest_tracks_output_digests_and_preserves_source_identity() {
        // Arrange
        let fixture = Fixture::new();
        let original = fixture.manifest();
        let mut bundle = verified_bundle(&fixture.0).unwrap();
        let profile = profile(ProfileVariant::Organization);

        // Act
        project_bundle(&mut bundle, &profile).unwrap();
        let projected: RuntimeManifest =
            serde_json::from_slice(member(&bundle, "runtime-manifest.json")).unwrap();

        // Assert
        assert_eq!(projected.source_revision, original.source_revision);
        assert_eq!(projected.source_fingerprint, original.source_fingerprint);
        assert_eq!(projected.generator_version, original.generator_version);
        assert_eq!(projected.files.len(), original.files.len());
        assert_ne!(projected.files["index.html"], original.files["index.html"]);
        assert_eq!(
            projected.files[WASM_FILES[1]],
            original.files[WASM_FILES[1]]
        );
        assert!(
            projected
                .files
                .iter()
                .all(|(name, digest)| sha256_bytes(member(&bundle, name)) == *digest)
        );
        assert_eq!(fixture.manifest().files, original.files);
    }

    #[test]
    fn profile_projection_is_deterministic_for_the_same_authenticated_source() {
        // Arrange
        let fixture = Fixture::new();
        let mut first = verified_bundle(&fixture.0).unwrap();
        let mut second = verified_bundle(&fixture.0).unwrap();
        let profile = profile(ProfileVariant::Organization);

        // Act
        project_bundle(&mut first, &profile).unwrap();
        project_bundle(&mut second, &profile).unwrap();

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn authenticated_projection_is_staged_as_a_complete_generated_bundle() {
        // Arrange
        let fixture = Fixture::new();
        let consumer = Fixture::new();
        let mut transaction = Transaction::begin(&consumer.0).unwrap();
        let profile = profile(ProfileVariant::Organization);

        // Act
        stage(
            &mut transaction,
            &consumer.0,
            Path::new("docs"),
            Some(&fixture.0),
            false,
            &profile,
        )
        .unwrap();
        let report = transaction.commit().unwrap();
        let manifest: RuntimeManifest = serde_json::from_slice(
            &fs::read(consumer.0.join("docs/runtime-manifest.json")).unwrap(),
        )
        .unwrap();

        // Assert
        assert_eq!(report.written, 10);
        assert!(
            fs::read_to_string(consumer.0.join("docs/index.html"))
                .unwrap()
                .contains("SOURCEFIELD / example-labs")
        );
        assert!(manifest.files.iter().all(|(name, digest)| {
            sha256_bytes(&fs::read(consumer.0.join("docs").join(name)).unwrap()) == *digest
        }));
    }

    #[test]
    fn tampered_source_cannot_reach_projection_or_stage_any_output() {
        // Arrange
        let fixture = Fixture::new();
        let consumer = Fixture::new();
        fs::create_dir(consumer.0.join("docs")).unwrap();
        fs::write(consumer.0.join("docs/index.html"), b"previous profile").unwrap();
        let altered = b"<html>modified source bundle</html>";
        fs::write(fixture.0.join("index.html"), altered).unwrap();
        let mut manifest = fixture.manifest();
        manifest
            .files
            .insert("index.html".into(), sha256_bytes(altered));
        fixture.write_manifest(&manifest);
        let mut transaction = Transaction::begin(&consumer.0).unwrap();
        let profile = profile(ProfileVariant::Organization);

        // Act
        let result = stage(
            &mut transaction,
            &consumer.0,
            Path::new("docs"),
            Some(&fixture.0),
            true,
            &profile,
        );
        let report = transaction.commit().unwrap();

        // Assert
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("authored asset differs")
        );
        assert_eq!(report.written, 0);
        assert_eq!(
            fs::read(consumer.0.join("docs/index.html")).unwrap(),
            b"previous profile"
        );
    }

    #[test]
    fn template_changes_cannot_silently_omit_profile_identity() {
        // Arrange
        let fixture = Fixture::new();
        let mut bundle = verified_bundle(&fixture.0).unwrap();
        let html = bundle
            .iter_mut()
            .find(|(name, _)| name == "index.html")
            .unwrap();
        html.1 = b"<html lang=\"en\"><title>Changed template</title></html>".to_vec();
        let profile = profile(ProfileVariant::Personal);

        // Act
        let result = project_bundle(&mut bundle, &profile);

        // Assert
        assert!(result.unwrap_err().to_string().contains("template marker"));
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
