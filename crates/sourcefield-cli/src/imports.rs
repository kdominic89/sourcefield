//! Resolve canonical organization manifests with immutable, replayable provenance.

use std::{collections::BTreeSet, path::Path, time::Duration};

use crate::modes::ExecutionMode;
use anyhow::{Context, Result, bail, ensure};
use reqwest::{
    Client, Url,
    header::{ACCEPT, AUTHORIZATION, USER_AGENT},
};
use serde::{Deserialize, Serialize};
use sourcefield_core::{
    Config, LayoutAssignments, OrganizationImport, OrganizationSource, compose_organizations,
    parse_organization,
};

const MANIFEST_LIMIT: usize = 256 * 1024;
const IMPORT_LIMIT: usize = 32;
const CAPTURE_SCHEMA: u32 = 1;
const GITHUB_API: &str = "https://api.github.com/";

/// Persisted canonical inputs sufficient to replay imports without network access.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportCapture {
    /// Version of this capture envelope, independent from generator SemVer.
    pub schema_version: u32,
    /// Ordered organization inputs, including their authored source declarations.
    pub imports: Vec<CapturedImport>,
}

/// One exact manifest and the identity of the source from which it was obtained.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedImport {
    /// Consumer-selected stable organization namespace.
    pub id: String,
    /// Source declaration used to resolve this input.
    pub source: OrganizationSource,
    /// Resolved remote or clean first-party commit; absent for local development input.
    pub commit: Option<String>,
    /// Verified repository identity, absent for uncommitted local development input.
    pub repository: Option<String>,
    /// Lowercase SHA-256 of the exact UTF-8 manifest bytes.
    pub digest: String,
    /// Captured canonical facts; credentials and HTTP diagnostics are never stored.
    pub manifest: String,
}

/// Import execution boundaries supplied by the CLI's staging transaction.
pub struct ResolveOptions<'a> {
    /// Consumer configuration directory against which local paths are resolved.
    pub root: &'a Path,
    /// Prior stable slot assignments retained while composing new organization facts.
    pub previous: &'a LayoutAssignments,
    /// Previously captured input set for locked or offline replay.
    pub captured: Option<&'a ImportCapture>,
    /// Validated execution intent controlling network, replay, and publication boundaries.
    pub mode: ExecutionMode,
    /// Optional GitHub credential; used only with the fixed GitHub API origin.
    pub token: Option<&'a str>,
}

/// Fully composed facts and reproducibility inputs for the caller's staged write.
#[derive(Debug)]
pub struct ResolvedImports {
    /// Validated configuration with canonical organization facts composed into it.
    pub config: Config,
    /// Stable layout assignments to persist alongside captured inputs.
    pub layout: LayoutAssignments,
    /// Captured input vector in authored organization order.
    pub capture: ImportCapture,
}

/// Resolve every configured organization before returning any candidate output.
///
/// Locked mode requires offline replay. Failure never mutates files; the caller
/// owns the complete output transaction and retains its previous published state.
pub async fn resolve(config: &Config, options: ResolveOptions<'_>) -> Result<ResolvedImports> {
    resolve_at(config, options, GITHUB_API).await
}

/// An injectable API origin is private so production callers cannot redirect credentials.
async fn resolve_at(
    config: &Config,
    options: ResolveOptions<'_>,
    api: &str,
) -> Result<ResolvedImports> {
    ensure!(
        config.imports.len() <= IMPORT_LIMIT,
        "too many organization imports"
    );

    if options.mode.is_offline()
        && let Some(capture) = options.captured
    {
        validate_capture(capture)?;
    }

    if options.mode.is_replay() {
        let capture = options
            .captured
            .context("locked imports require captured inputs")?;

        ensure!(
            capture.imports.len() == config.imports.len(),
            "captured import inventory differs"
        );
    }

    // A replay never even constructs an HTTP client, making the offline boundary explicit.
    let client = if options.mode.is_offline() {
        None
    } else {
        Some(
            Client::builder()
                .timeout(Duration::from_secs(24))
                .connect_timeout(Duration::from_secs(8))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| anyhow::anyhow!("cannot initialize import transport"))?,
        )
    };

    let mut inputs = Vec::with_capacity(config.imports.len());
    let mut manifests = Vec::with_capacity(config.imports.len());
    let mut ids = BTreeSet::new();

    for import in &config.imports {
        ensure!(ids.insert(&import.id), "duplicate organization import ID");

        let input = if options.mode.is_replay()
            || (options.mode.is_offline()
                && matches!(import.source, OrganizationSource::Remote { .. }))
        {
            let capture = options
                .captured
                .context("offline remote imports require captured inputs")?;

            let saved = capture
                .imports
                .iter()
                .find(|saved| saved.id == import.id)
                .context("organization missing from captured inputs")?;

            verify_saved(import, saved)?;
            saved.clone()
        } else {
            match &import.source {
                OrganizationSource::Local { path } => {
                    let manifest = read_local(options.root, path)?;

                    let identity = if options.mode.requires_published_imports() {
                        Some(verify_first_party(config, options.root, path, &manifest)?)
                    } else {
                        None
                    };

                    CapturedImport {
                        id: import.id.clone(),
                        source: import.source.clone(),
                        repository: identity.as_ref().map(|(repository, _)| repository.clone()),
                        commit: identity.map(|(_, commit)| commit),
                        digest: digest(&manifest),
                        manifest,
                    }
                }
                OrganizationSource::Remote { .. } => fetch_remote(
                    client
                        .as_ref()
                        .context("network forbidden in offline mode")?,
                    import,
                    options.token,
                    api,
                )
                .await
                .with_context(|| {
                    format!(
                        "required remote organization import {:?} failed; observation fallback does not substitute configuration",
                        import.id
                    )
                })?,
            }
        };

        ensure!(
            !options.mode.requires_published_imports() || input.commit.is_some(),
            "local development imports cannot be published"
        );

        let manifest = parse_organization(&input.manifest)
            .map_err(|_| anyhow::anyhow!("invalid canonical organization manifest"))?;

        if let OrganizationSource::Remote { repository, .. } = &import.source {
            let owner = repository
                .split('/')
                .next()
                .context("import repository has no owner")?;

            ensure!(
                manifest.owner.eq_ignore_ascii_case(owner),
                "canonical organization owner differs from source repository"
            );
        }

        if options.mode.requires_published_imports()
            && matches!(import.source, OrganizationSource::Local { .. })
        {
            ensure!(
                config.profile.variant == sourcefield_core::ProfileVariant::Organization
                    && manifest
                        .owner
                        .eq_ignore_ascii_case(&config.profile.organization),
                "local canonical source does not belong to this organization profile"
            );
            let repository = input
                .repository
                .as_deref()
                .context("local canonical source has no repository identity")?;

            ensure!(
                repository
                    .eq_ignore_ascii_case(&format!("{}/.github", config.profile.organization)),
                "local canonical source repository differs from profile organization"
            );
        }

        ensure!(
            manifest.id == import.id,
            "organization manifest namespace differs from import ID"
        );
        manifests.push(manifest);
        inputs.push(input);
    }

    let (config, layout) = compose_organizations(config, &manifests, options.previous)?;

    Ok(ResolvedImports {
        config,
        layout,
        capture: ImportCapture {
            schema_version: CAPTURE_SCHEMA,
            imports: inputs,
        },
    })
}

/// Check capture bounds before cloning payloads or interpreting their contents.
fn validate_capture(capture: &ImportCapture) -> Result<()> {
    ensure!(
        capture.schema_version == CAPTURE_SCHEMA,
        "unsupported import capture schema"
    );
    ensure!(
        capture.imports.len() <= IMPORT_LIMIT,
        "too many captured imports"
    );

    let mut ids = BTreeSet::new();

    for input in &capture.imports {
        ensure!(ids.insert(&input.id), "duplicate captured import ID");
        ensure!(
            input.manifest.len() <= MANIFEST_LIMIT,
            "captured manifest exceeds size limit"
        );
    }

    Ok(())
}

/// Bind replay to both the source declaration and the exact captured bytes.
fn verify_saved(import: &OrganizationImport, saved: &CapturedImport) -> Result<()> {
    ensure!(
        serde_json::to_value(&import.source)? == serde_json::to_value(&saved.source)?,
        "captured import source differs"
    );
    ensure!(
        saved.digest == digest(&saved.manifest),
        "captured manifest digest mismatch"
    );

    match &import.source {
        OrganizationSource::Local { .. } => {
            ensure!(
                saved.commit.is_some() == saved.repository.is_some(),
                "local input claims remote provenance"
            );

            if let Some(commit) = saved.commit.as_deref() {
                validate_commit(commit, "HEAD")?;
            }
        }
        OrganizationSource::Remote {
            reference,
            repository,
            ..
        } => {
            ensure!(
                saved.repository.as_deref() == Some(repository.as_str()),
                "captured repository differs"
            );
            let commit = saved
                .commit
                .as_deref()
                .context("remote capture has no commit")?;

            validate_commit(commit, reference)?;
        }
    }

    Ok(())
}

/// Stream a local file with the same upper bound enforced on HTTP input.
fn read_local(root: &Path, path: &str) -> Result<String> {
    ensure!(!path.is_empty(), "local manifest path is empty");

    let candidate = root.join(path);
    let file = std::fs::File::open(candidate).context("cannot read local organization manifest")?;
    sourcefield_io::read_utf8_bounded(file, MANIFEST_LIMIT as u64)
        .context("cannot read bounded UTF-8 organization manifest")
}

/// Authenticate an organization's own checked-out canonical file without fetching or mutating Git.
fn verify_first_party(
    config: &Config,
    root: &Path,
    path: &str,
    manifest: &str,
) -> Result<(String, String)> {
    ensure!(
        config.profile.variant == sourcefield_core::ProfileVariant::Organization,
        "personal profiles cannot publish local organization imports"
    );

    let checkout = git_text(root, &["rev-parse", "--show-toplevel"])?;
    let checkout =
        std::fs::canonicalize(checkout.trim()).context("cannot resolve canonical checkout")?;

    let candidate = root.join(path);
    let resolved =
        std::fs::canonicalize(&candidate).context("cannot resolve canonical manifest")?;

    let relative = resolved
        .strip_prefix(&checkout)
        .context("canonical manifest is outside checkout")?;

    // A symlink could race between reading bytes and proving Git identity; refuse it in canonical publication.
    let mut ancestor = candidate.as_path();

    while ancestor != root && ancestor.starts_with(root) {
        ensure!(
            !std::fs::symlink_metadata(ancestor)?
                .file_type()
                .is_symlink(),
            "canonical manifest path contains a symlink"
        );
        ancestor = ancestor.parent().context("invalid canonical path")?;
    }

    let origin = git_text(root, &["remote", "get-url", "origin"])?;
    let repository = github_repository(origin.trim())?;
    ensure!(
        repository.eq_ignore_ascii_case(&format!("{}/.github", config.profile.organization)),
        "canonical checkout origin differs from organization profile"
    );

    let commit = git_text(root, &["rev-parse", "HEAD"])?;
    let commit = commit.trim().to_owned();
    validate_commit(&commit, "HEAD")?;

    let tree_path = relative
        .to_str()
        .context("canonical path is not UTF-8")?
        .replace('\\', "/");

    let object = format!("{commit}:{tree_path}");
    let size = git_text(root, &["cat-file", "-s", &object])?;
    ensure!(
        size.trim().parse::<usize>()? <= MANIFEST_LIMIT,
        "committed manifest exceeds size limit"
    );

    let tracked = git_bytes(root, &["show", &object])?;
    ensure!(
        tracked == manifest.as_bytes(),
        "canonical manifest differs from committed HEAD"
    );

    Ok((repository, commit))
}

/// Accept only ordinary GitHub origin forms, excluding credentials embedded in HTTPS URLs.
fn github_repository(origin: &str) -> Result<String> {
    let repository = origin
        .strip_prefix("https://github.com/")
        .or_else(|| origin.strip_prefix("git@github.com:"))
        .or_else(|| origin.strip_prefix("ssh://git@github.com/"))
        .context("canonical origin must identify GitHub")?
        .trim_end_matches('/')
        .trim_end_matches(".git");

    let parts = repository.split('/').collect::<Vec<_>>();
    ensure!(
        parts.len() == 2 && parts.iter().all(|part| valid_component(part)),
        "invalid canonical repository identity"
    );

    Ok(repository.into())
}

fn git_text(root: &Path, arguments: &[&str]) -> Result<String> {
    String::from_utf8(git_bytes(root, arguments)?).context("Git metadata is not UTF-8")
}

fn git_bytes(root: &Path, arguments: &[&str]) -> Result<Vec<u8>> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .context("cannot inspect canonical checkout")?;

    ensure!(
        output.status.success(),
        "canonical checkout identity or tracked manifest unavailable"
    );
    ensure!(
        output.stdout.len() <= MANIFEST_LIMIT,
        "canonical checkout metadata exceeds size limit"
    );

    Ok(output.stdout)
}

/// Resolve a branch once, then fetch content exclusively through the immutable commit.
async fn fetch_remote(
    client: &Client,
    import: &OrganizationImport,
    token: Option<&str>,
    api: &str,
) -> Result<CapturedImport> {
    let OrganizationSource::Remote {
        repository,
        reference,
        path,
    } = &import.source
    else {
        bail!("remote resolver received local input");
    };

    let parts: Vec<_> = repository.split('/').collect();
    ensure!(
        parts.len() == 2 && parts.iter().all(|part| valid_component(part)),
        "invalid import repository"
    );
    ensure!(
        !reference.is_empty() && reference.len() <= 255,
        "invalid import ref"
    );
    ensure!(
        !path.is_empty()
            && !path.starts_with('/')
            && path
                .split('/')
                .all(|part| part != "." && part != ".." && !part.is_empty()),
        "invalid remote manifest path"
    );

    let mut commit_url = Url::parse(api).context("invalid GitHub API origin")?;
    commit_url
        .path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid API origin"))?
        .extend(["repos", parts[0], parts[1], "commits", reference]);

    // GitHub's SHA media type omits commit messages, author data, and the potentially large diff.
    let bytes = request(client, commit_url, token, "application/vnd.github.sha").await?;
    let text = std::str::from_utf8(&bytes).context("commit response is not UTF-8")?;
    let commit = text.trim();

    validate_commit(commit, reference)?;

    let mut contents_url = Url::parse(api).context("invalid GitHub API origin")?;
    contents_url
        .path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid API origin"))?
        .extend(["repos", parts[0], parts[1], "contents"])
        .extend(path.split('/'));
    contents_url.query_pairs_mut().append_pair("ref", commit);

    // The contents API's JSON `sha` is a blob ID, so it must never substitute for commit provenance.
    let bytes = request(
        client,
        contents_url,
        token,
        "application/vnd.github.raw+json",
    )
    .await?;

    let manifest = String::from_utf8(bytes).context("organization manifest is not UTF-8")?;

    Ok(CapturedImport {
        id: import.id.clone(),
        source: import.source.clone(),
        commit: Some(commit.into()),
        repository: Some(repository.clone()),
        digest: digest(&manifest),
        manifest,
    })
}

/// Read bounded bytes without retaining upstream bodies or URL-bearing transport errors.
async fn request(
    client: &Client,
    url: Url,
    token: Option<&str>,
    accept: &'static str,
) -> Result<Vec<u8>> {
    let mut request = client
        .get(url)
        .header(USER_AGENT, "sourcefield-imports")
        .header(ACCEPT, accept)
        .header("X-GitHub-Api-Version", "2022-11-28");

    if let Some(token) = token.filter(|token| !token.trim().is_empty()) {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let mut response = sourcefield_collector::send_read_request(request)
        .await
        .map_err(|_| anyhow::anyhow!("import transport failed"))?;
    let limit = if accept == "application/vnd.github.sha" {
        64
    } else {
        MANIFEST_LIMIT
    };

    ensure!(
        response.status().is_success(),
        "import upstream returned HTTP {}",
        response.status().as_u16()
    );
    ensure!(
        !response
            .content_length()
            .is_some_and(|length| length > limit as u64),
        "import response exceeds size limit"
    );

    let mut bytes = Vec::new();

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("import transport failed"))?
    {
        ensure!(
            chunk.len() <= limit.saturating_sub(bytes.len()),
            "import response exceeds size limit"
        );
        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes)
}

fn validate_commit(commit: &str, reference: &str) -> Result<()> {
    ensure!(is_commit(commit), "invalid resolved commit SHA");
    ensure!(
        !is_commit(reference) || commit.eq_ignore_ascii_case(reference),
        "resolved commit differs from pinned ref"
    );

    Ok(())
}

fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn digest(value: &str) -> String {
    sourcefield_io::sha256_bytes(value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
    const MANIFEST: &str = "schema_version = 1\nid = 'example'\nowner = 'example-labs'\nlabel = 'Example'\nsummary = 'Public tools'\n";

    fn config() -> Config {
        serde_json::from_value(serde_json::json!({
            "schema_version":1,
            "profile":{
                "variant":"personal", "username":"owner", "organization":"example-labs",
                "display_name":"Owner", "headline":"Tools", "tagline":"Tools",
                "pages_url":"https://example.invalid", "source_url":"https://github.com/owner/profile"
            },
            "collection":{"github_user":"owner","collect_contributions":false},
            "render":{"width":1200,"height":1400}
        })).unwrap()
    }

    fn remote() -> OrganizationImport {
        OrganizationImport {
            id: "example".into(),
            source: OrganizationSource::Remote {
                repository: "example-labs/.github".into(),
                reference: "main".into(),
                path: "config/organization.toml".into(),
            },
        }
    }

    fn saved() -> CapturedImport {
        CapturedImport {
            id: "example".into(),
            source: remote().source,
            commit: Some(COMMIT.into()),
            repository: Some("example-labs/.github".into()),
            digest: digest(MANIFEST),
            manifest: MANIFEST.into(),
        }
    }

    fn options<'a>(
        layout: &'a LayoutAssignments,
        capture: Option<&'a ImportCapture>,
    ) -> ResolveOptions<'a> {
        ResolveOptions {
            root: Path::new("."),
            previous: layout,
            captured: capture,
            mode: ExecutionMode::LockedReplay,
            token: Some("never-send-this"),
        }
    }

    /// Real HTTP fixture captures only test credentials and bounds each connection's wait.
    fn server(
        responses: Vec<(u16, String)>,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let worker = thread::spawn(move || {
            for (status, body) in responses {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "missing fixture request"
                            );
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("fixture accept failed: {error}"),
                    }
                };

                // Accepted sockets can inherit nonblocking mode on macOS. Use bounded blocking
                // reads so a request arriving just after accept is not mistaken for a failure.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut buffer = [0; 8192];
                let length = stream.read(&mut buffer).unwrap();
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[..length]).into_owned());
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });

        (endpoint, requests, worker)
    }

    #[tokio::test]
    async fn permissive_remote_failure_does_not_substitute_a_captured_manifest() {
        let mut config = config();
        config.imports.push(remote());
        let capture = ImportCapture {
            schema_version: 1,
            imports: vec![saved()],
        };
        let layout = LayoutAssignments::new();
        let (api, requests, worker) = server(vec![(404, "never-expose-response-body".into())]);
        let mut policy = options(&layout, Some(&capture));
        policy.mode = ExecutionMode::Refresh(crate::modes::FailurePolicy::AllowFallback);

        let result = resolve_at(&config, policy, &api).await;

        worker.join().unwrap();
        let message = format!("{:#}", result.unwrap_err());
        assert!(
            message.contains("required remote organization import"),
            "{message}"
        );
        assert!(message.contains("observation fallback"), "{message}");
        assert!(message.contains("example"), "{message}");
        assert!(!message.contains("never-expose-response-body"));
        assert!(!message.contains("never-send-this"));
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert_eq!(capture.imports[0].digest, digest(MANIFEST));
        assert!(layout.is_empty());
    }

    #[tokio::test]
    async fn immutable_fetch_uses_resolved_commit_for_raw_contents() {
        let (api, requests, worker) = server(vec![(200, COMMIT.into()), (200, MANIFEST.into())]);

        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let input = fetch_remote(&client, &remote(), Some("test-token"), &api)
            .await
            .unwrap();

        worker.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /repos/example-labs/.github/commits/main "));
        assert!(requests[0].contains("application/vnd.github.sha"));
        assert!(requests[1].contains(&format!("/contents/config/organization.toml?ref={COMMIT}")));
        assert!(requests[1].contains("application/vnd.github.raw+json"));
        assert_eq!(input.commit.as_deref(), Some(COMMIT));
        assert_eq!(input.digest, digest(MANIFEST));
        assert!(
            !serde_json::to_string(&input)
                .unwrap()
                .contains("test-token")
        );
    }

    #[tokio::test]
    async fn offline_replay_succeeds_with_unreachable_transport_origin() {
        let mut config = config();
        config.imports.push(remote());
        let capture = ImportCapture {
            schema_version: 1,
            imports: vec![saved()],
        };

        let layout = LayoutAssignments::new();

        let result = resolve_at(
            &config,
            options(&layout, Some(&capture)),
            "http://127.0.0.1:1/",
        )
        .await
        .unwrap();

        assert_eq!(result.config.domains[0].id, "example");
        assert_eq!(result.capture.imports[0].digest, digest(MANIFEST));
    }

    #[tokio::test]
    async fn offline_missing_capture_fails_without_network() {
        let mut config = config();
        config.imports.push(remote());
        let layout = LayoutAssignments::new();

        let result = resolve_at(&config, options(&layout, None), "http://127.0.0.1:1/").await;

        assert!(result.unwrap_err().to_string().contains("captured inputs"));
    }

    #[test]
    fn digest_mismatch_is_rejected() {
        let mut capture = saved();
        capture.manifest.push_str("# changed");

        let result = verify_saved(&remote(), &capture);

        assert!(result.unwrap_err().to_string().contains("digest mismatch"));
    }

    #[test]
    fn changed_source_is_rejected_even_when_digest_matches() {
        let mut import = remote();
        import.source = OrganizationSource::Remote {
            repository: "different/.github".into(),
            reference: "main".into(),
            path: "config/organization.toml".into(),
        };

        let result = verify_saved(&import, &saved());

        assert!(result.unwrap_err().to_string().contains("source differs"));
    }

    #[test]
    fn pinned_commit_mismatch_is_rejected() {
        let expected = "abcdef0123456789abcdef0123456789abcdef01";

        let result = validate_commit(COMMIT, expected);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("differs from pinned ref")
        );
    }

    #[test]
    fn blob_or_invalid_commit_identity_is_rejected() {
        let result = validate_commit("not-a-commit", "main");

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid resolved commit")
        );
    }

    #[test]
    fn local_capture_cannot_claim_remote_commit() {
        let import = OrganizationImport {
            id: "example".into(),
            source: OrganizationSource::Local {
                path: "organization.toml".into(),
            },
        };

        let mut capture = saved();
        capture.source = import.source.clone();
        capture.repository = None;

        let result = verify_saved(&import, &capture);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("claims remote provenance")
        );
    }

    #[tokio::test]
    async fn cross_organization_manifest_is_rejected() {
        let mut config = config();
        config.imports.push(remote());
        let mut input = saved();
        input.manifest = MANIFEST.replace("id = 'example'", "id = 'other'");
        input.digest = digest(&input.manifest);
        let capture = ImportCapture {
            schema_version: 1,
            imports: vec![input],
        };

        let layout = LayoutAssignments::new();

        let result = resolve(&config, options(&layout, Some(&capture))).await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("namespace differs")
        );
    }

    #[tokio::test]
    async fn malformed_manifest_is_rejected_without_exposing_body() {
        let mut config = config();
        config.imports.push(remote());
        let mut input = saved();
        input.manifest = "SECRET-CONTENT".into();
        input.digest = digest(&input.manifest);
        let capture = ImportCapture {
            schema_version: 1,
            imports: vec![input],
        };

        let layout = LayoutAssignments::new();

        let result = resolve(&config, options(&layout, Some(&capture))).await;

        assert_eq!(
            result.unwrap_err().to_string(),
            "invalid canonical organization manifest"
        );
    }

    #[test]
    fn duplicate_capture_id_is_rejected() {
        let capture = ImportCapture {
            schema_version: 1,
            imports: vec![saved(), saved()],
        };

        let result = validate_capture(&capture);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("duplicate captured")
        );
    }

    #[test]
    fn oversized_capture_is_rejected() {
        let mut input = saved();
        input.manifest = "a".repeat(MANIFEST_LIMIT + 1);
        let capture = ImportCapture {
            schema_version: 1,
            imports: vec![input],
        };

        let result = validate_capture(&capture);

        assert!(result.unwrap_err().to_string().contains("size limit"));
    }

    #[tokio::test]
    async fn oversized_remote_response_is_rejected() {
        let (api, _, worker) = server(vec![(200, "a".repeat(MANIFEST_LIMIT + 1))]);
        let client = Client::new();

        let result = request(&client, Url::parse(&api).unwrap(), None, "application/json").await;

        worker.join().unwrap();
        assert!(result.unwrap_err().to_string().contains("size limit"));
    }

    #[tokio::test]
    async fn upstream_failure_body_is_redacted() {
        let (api, _, worker) = server(vec![(403, "SECRET-UPSTREAM-BODY".into())]);
        let client = Client::new();

        let result = request(&client, Url::parse(&api).unwrap(), None, "application/json").await;

        worker.join().unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "import upstream returned HTTP 403"
        );
    }

    #[test]
    fn local_paths_are_resolved_relative_to_consumer_root() {
        let root = std::env::temp_dir().join(format!("sourcefield-import-{}", std::process::id()));
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("config/organization.toml"), MANIFEST).unwrap();

        let result = read_local(&root, "config/organization.toml");

        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(result.unwrap(), MANIFEST);
    }
    /// Create isolated committed test data without touching any working repository.
    fn canonical_checkout() -> (std::path::PathBuf, Config) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "sourcefield-canonical-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));

        std::fs::create_dir_all(&root).unwrap();
        let status = std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&root)
            .status()
            .unwrap();

        assert!(status.success());
        let stream = format!(
            "blob\nmark :1\ndata {}\n{}\ncommit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1 +0000\ndata 7\nfixture\nM 100644 :1 organization.toml\n\ndone\n",
            MANIFEST.len(),
            MANIFEST
        );

        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["fast-import", "--quiet"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        child
            .stdin
            .take()
            .unwrap()
            .write_all(stream.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["checkout", "-q", "main"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/example-labs/.github.git"
                ])
                .status()
                .unwrap()
                .success()
        );
        let mut config = config();
        config.profile.variant = sourcefield_core::ProfileVariant::Organization;

        (root, config)
    }

    #[test]
    fn clean_first_party_manifest_has_commit_and_repository_provenance() {
        let (root, config) = canonical_checkout();

        let result = verify_first_party(&config, &root, "organization.toml", MANIFEST);

        std::fs::remove_dir_all(root).unwrap();
        let (repository, commit) = result.unwrap();
        assert_eq!(repository, "example-labs/.github");
        assert!(is_commit(&commit));
    }

    #[test]
    fn dirty_first_party_manifest_cannot_claim_committed_identity() {
        let (root, config) = canonical_checkout();
        let changed = format!("{MANIFEST}# changed\n");
        std::fs::write(root.join("organization.toml"), &changed).unwrap();

        let result = verify_first_party(&config, &root, "organization.toml", &changed);

        std::fs::remove_dir_all(root).unwrap();
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("differs from committed HEAD")
        );
    }

    #[test]
    fn untracked_first_party_manifest_is_rejected() {
        let (root, config) = canonical_checkout();
        std::fs::write(root.join("untracked.toml"), MANIFEST).unwrap();

        let result = verify_first_party(&config, &root, "untracked.toml", MANIFEST);

        std::fs::remove_dir_all(root).unwrap();
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("tracked manifest unavailable")
        );
    }

    #[test]
    fn different_org_checkout_cannot_publish_first_party_manifest() {
        let (root, mut config) = canonical_checkout();
        config.profile.organization = "other-labs".into();

        let result = verify_first_party(&config, &root, "organization.toml", MANIFEST);

        std::fs::remove_dir_all(root).unwrap();
        assert!(result.unwrap_err().to_string().contains("origin differs"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_first_party_manifest_is_rejected() {
        let (root, config) = canonical_checkout();
        std::os::unix::fs::symlink("organization.toml", root.join("linked.toml")).unwrap();

        let result = verify_first_party(&config, &root, "linked.toml", MANIFEST);

        std::fs::remove_dir_all(root).unwrap();
        assert!(result.unwrap_err().to_string().contains("symlink"));
    }
    #[tokio::test]
    async fn redirect_cannot_forward_github_credentials_to_another_origin() {
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let destination = format!("http://{}/", target.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0; 8192];
            let _ = stream.read(&mut bytes).unwrap();
            write!(stream, "HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });

        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let result = request(&client, url, Some("private-credential"), "application/json").await;

        worker.join().unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "import upstream returned HTTP 302"
        );
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    #[tokio::test]
    async fn remote_manifest_cannot_claim_another_repository_owner() {
        let mut config = config();
        config.imports.push(remote());
        let mut input = saved();
        input.manifest = MANIFEST.replace("owner = 'example-labs'", "owner = 'other-labs'");
        input.digest = digest(&input.manifest);
        let capture = ImportCapture {
            schema_version: 1,
            imports: vec![input],
        };

        let layout = LayoutAssignments::new();

        let result = resolve(&config, options(&layout, Some(&capture))).await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("owner differs from source repository")
        );
    }
    #[tokio::test]
    async fn sha_media_response_rejects_full_commit_json() {
        let (api, _, worker) = server(vec![(200, format!("{{\"sha\":\"{COMMIT}\"}}"))]);
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let result = fetch_remote(&client, &remote(), None, &api).await;

        worker.join().unwrap();
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid resolved commit SHA")
        );
    }

    #[tokio::test]
    async fn sha_media_response_has_a_smaller_bound_than_manifests() {
        let (api, _, worker) = server(vec![(200, "a".repeat(65))]);
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let result = fetch_remote(&client, &remote(), None, &api).await;

        worker.join().unwrap();
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("response exceeds size limit")
        );
    }
}
