# Primary implementation sources

The initial references were checked on 2026-10-04; later sections record their own verification dates.
These references establish external API behavior, not evidence that a hosted release or deployment
has already run.

- Reusable workflows and immutable commit references: https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows
- Caller/callee permissions: https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations
- Reusable job identity: https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#example-usage-of-job-context-workflow-identity
- Immutable releases: https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases
- Release integrity verification: https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/verify-release-integrity
- Build provenance: https://docs.github.com/en/actions/concepts/security/artifact-attestations
- Attestation verification constraints: https://cli.github.com/manual/gh_attestation_verify
- Immutable-release settings and required administration-read permission: https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository
- Native runner labels and architectures: https://docs.github.com/en/actions/reference/runners/github-hosted-runners
- Concurrency: https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency
- Commit-pinned repository contents: https://docs.github.com/en/rest/repos/contents#get-repository-content
- Reference resolution: https://docs.github.com/en/rest/commits/commits#get-a-commit
- Strict Serde models: https://serde.rs/container-attrs.html
- TOML: https://toml.io/en/
- Semantic versioning: https://semver.org/

Workflow action revisions are full commit pins. Update them through a reviewed dependency change and
verify vendor release provenance; major-version labels in comments are explanatory only. Rust and
WASM compiler/binding versions are pinned in `rust-toolchain.toml` and the root Cargo workspace;
`tools/binaryen.json` pins the official optimizer version and authenticated host artifacts.
The current Playwright pin and dependency closure live in `tools/browser/package.json` and
`tools/browser/package-lock.json`; dated registry observations below are historical evidence.

- Playwright CI/browser provisioning: https://playwright.dev/docs/ci
- Existing browser test tool: [authoritative manifest](tools/browser/package.json) and
  [official registry metadata](https://registry.npmjs.org/playwright); current dated versions are below.

- Dependabot ecosystems, weekly schedules and grouped Actions updates: https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference

## Dependency freshness snapshot

Before choosing or refreshing pins, inspect the latest non-yanked stable crate releases, the stable
Rust distribution manifest, and the official tool/Action releases. Record the retrieval date and
resolved Action commit; a successful build alone does not establish dependency freshness. Recheck
these sources before proposing publication. Future releases can make this dated snapshot obsolete.

Verified on 2026-10-31:

| Direct crate | Exact pin | Primary release metadata |
| --- | --- | --- |
| anyhow | 1.0.104 | https://crates.io/api/v1/crates/anyhow |
| chrono | 0.4.45 | https://crates.io/api/v1/crates/chrono |
| clap | 4.6.7 | https://crates.io/api/v1/crates/clap |
| hex | 0.4.3 | https://crates.io/api/v1/crates/hex |
| reqwest | 0.13.5 | https://crates.io/api/v1/crates/reqwest |
| serde | 1.0.229 | https://crates.io/api/v1/crates/serde |
| serde_json | 1.0.154 | https://crates.io/api/v1/crates/serde_json |
| sha2 | 0.11.1 | https://crates.io/api/v1/crates/sha2 |
| thiserror | 2.0.21 | https://crates.io/api/v1/crates/thiserror |
| tokio | 1.54.1 | https://crates.io/api/v1/crates/tokio |
| toml | 1.1.8+spec-1.1.0 | https://crates.io/api/v1/crates/toml |
| wasm-bindgen | 0.2.130 | https://crates.io/api/v1/crates/wasm-bindgen |
| wasm-bindgen-cli-support | 0.2.130 | https://crates.io/api/v1/crates/wasm-bindgen-cli-support |

Cargo's TOML requirement uses `=1.1.8` because build metadata is ignored in version requirements;
the lockfile retains the complete published version. Reqwest 0.13 moves query encoding behind the
`query` feature and selects Rustls with `rustls`; only `json`, `query` and `rustls` are enabled here.
The Rustls backend now uses reqwest's default AWS-LC provider and platform certificate verifier.
Its transitive libraries are resolved in `Cargo.lock`; no separate direct TLS dependency is added.

- Reqwest 0.13 changes: https://github.com/seanmonstar/reqwest/releases/tag/v0.13.0
- Rust 1.99.1 stable distribution: https://static.rust-lang.org/dist/channel-rust-stable.toml
- Binaryen version_133: https://github.com/WebAssembly/binaryen/releases/tag/version_133
- actions/checkout v7.0.1: https://github.com/actions/checkout/releases/tag/v7.0.1
- actions/upload-artifact v7.0.2: https://github.com/actions/upload-artifact/releases/tag/v7.0.2
- actions/download-artifact v8.0.2: https://github.com/actions/download-artifact/releases/tag/v8.0.2
- actions/attest v4.2.2: https://github.com/actions/attest/releases/tag/v4.2.2
- Direct attestation action recommendation: https://github.com/actions/attest-build-provenance/blob/v4.2.2/README.md
- actions/upload-pages-artifact v5.0.0: https://github.com/actions/upload-pages-artifact/releases/tag/v5.0.0
- actions/deploy-pages v5.0.1: https://github.com/actions/deploy-pages/releases/tag/v5.0.1
- Ubuntu runner-installed browsers: https://github.com/actions/runner-images/blob/main/images/ubuntu/Ubuntu2404-Readme.md
- Python explicit text-file newlines: https://docs.python.org/3/library/pathlib.html#pathlib.Path.write_text
- Git text normalization: https://git-scm.com/docs/gitattributes

- Rustls platform certificate-store behavior: https://github.com/rustls/rustls-platform-verifier/blob/main/README.md
- AWS-LC native build requirements: https://aws.github.io/aws-lc-rs/requirements/index.html

The 2026-10-17 check included the browser tool and all six existing Action pins, including the
consumer-template Pages Actions. At that check, Playwright 1.64.1 was the current stable patch,
newer than the 1.64.0 Dependabot proposal. Its npm lock binds the exact playwright/playwright-core
tarball integrities; Node >=20 remains supported. All Action release commits above matched at that check.
sha2 0.11.1 requires Rust 1.88, below the workspace's existing 1.99 minimum. The selected toolchain
is the stable 1.99.1 patch; no minimum-version or feature expansion is needed.

- Playwright 1.64.1 package metadata: https://registry.npmjs.org/playwright/1.64.1
- Playwright core integrity metadata: https://registry.npmjs.org/playwright-core/1.64.1
- Playwright patch notes: https://github.com/microsoft/playwright/releases/tag/v1.64.1
- Chromium revision and version: https://github.com/microsoft/playwright/blob/v1.64.1/packages/playwright-core/browsers.json
- Rust patch announcement: https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/
- Minor/patch Dependabot groups: https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference#update-types-groups
- Published 0.1.2 identity and date: https://api.github.com/repos/kdominic89/sourcefield/releases/tags/v0.1.2

The dependency PR's actual Chromium 156 regression reached script policy evaluation during XML
parsing, before detached-tree event cleanup. The runtime now rejects event attributes at the same
quote-aware pre-parse boundary as inline styles. Unit and real-browser probes require no XML parse
for rejected event names, no executed handler, no external request and no unexpected CSP violation.
Observed browser behavior and locally executed verification remain separate from vendor API claims.

The separate local RustSec audit also identified RUSTSEC-2026-0346 through RUSTSEC-2026-0349
in the locked transitive Rustls 0.23.45, despite no open repository Dependabot alerts. Its targeted
lockfile update selects patched 0.23.46 under the unchanged Reqwest dependency and TLS features.
The advisory conditions differ; a vulnerable lockfile version alone is not proof that every
exploit path is enabled in this application. No custom client session store, explicit ECH mode,
or Rustls Brotli feature is introduced by the fix.

- Rustls registry and checksum: https://crates.io/api/v1/crates/rustls/0.23.46
- Client session debug redaction: https://rustsec.org/advisories/RUSTSEC-2026-0346.html
- ECH rejection ordering: https://rustsec.org/advisories/RUSTSEC-2026-0347.html
- Optional Brotli certificate memory bound: https://rustsec.org/advisories/RUSTSEC-2026-0348.html
- Authenticated key-share prediction: https://rustsec.org/advisories/RUSTSEC-2026-0349.html

## Dependency and certificate refresh (October 31, 2026)

The complete primary-source check covers all direct crate pins, the Cargo and browser lockfile
closures, the stable Rust distribution, the former wasm-pack tool, and all six Action release commits
including `docs/consumer-workflow.yml.template`. Rust 1.99.1, Playwright/core 1.64.1 and those Action commits
remain current. Node and Python are runner-provided tools with minimum requirements, not exact
repository release pins. Supported runner OS choices are separate from Action-version freshness.

Tokio 1.54.1 requires Rust 1.85 and includes the timer regression fix for the withdrawn 1.54.0
release. wasm-bindgen 0.2.130 and the retired wasm-pack 0.15.1 were published on October 30.
Keep the WASM dependency family and binding adapter aligned and regenerate native/browser bundles
from the final lockfile. Source fingerprints intentionally change even when authored profile content
is unchanged.
At the October 31 check, the workspace was preparing 0.1.3 and the latest published release was 0.1.2.

The wasm-pack 0.15.1 investigation found that its new target precedence honors `CARGO_BUILD_TARGET`
and Cargo `[build].target`; its `--target web` selects JavaScript output rather than the Rust target.
The replacement driver explicitly selects the browser and native adapter targets independently.

- Tokio release and MSRV: https://github.com/tokio-rs/tokio/releases/tag/tokio-1.54.0
- Tokio timer correction: https://github.com/tokio-rs/tokio/releases/tag/tokio-1.54.1
- wasm-bindgen release: https://github.com/wasm-bindgen/wasm-bindgen/releases/tag/0.2.130
- wasm-pack release: https://github.com/wasm-bindgen/wasm-pack/releases/tag/v0.15.1
- wasm-pack registry: https://crates.io/api/v1/crates/wasm-pack/0.15.1
- wasm-pack target precedence: https://github.com/wasm-bindgen/wasm-pack/blob/v0.15.1/src/command/build.rs

The local audit identifies RUSTSEC-2026-0382 in rustls-webpki 0.103.15 and RUSTSEC-2026-0383 in
webpki-root-certs 1.0.9. The compatible lock refresh selects corrected stable 0.103.16 and 1.0.11.
Both concern certificate paths through distrusted emSign intermediates. Keep the existing Reqwest
features, crypto provider and platform verifier; an affected lock entry alone does not establish
that every shipped target uses the bundled roots or the same certificate-path implementation.
The native platform verifier delegates to Apple/Windows trust APIs and uses WebPKI with native
roots on Linux/other supported Unix targets. The browser simulation crate has no network client.

- WebPKI certificate-path fix: https://rustsec.org/advisories/RUSTSEC-2026-0382.html
- Root-bundle correction: https://rustsec.org/advisories/RUSTSEC-2026-0383.html
- WebPKI implementation fix: https://github.com/rustls/webpki/pull/546
- Root removal: https://github.com/rustls/webpki-roots/pull/133
- Platform selection: https://github.com/rustls/rustls-platform-verifier/tree/v/0.7.1/src/verification
- Disclosure and offline certificate-chain fixtures: https://emudhra.we-have-a-problem.com/

Refresh versions admitted by the existing dependency graph; do not force older platform or
compatibility packages to unrelated major lines. A registry/solver comparison proves availability,
not successful compilation or security behavior. Audits, focused certificate checks, required
workspace/browser checks and real consumer acceptance provide separate verification evidence.

## Authenticated browser build tools (October 31, 2026)

At the October 31 check, the latest wasm-pack release was 0.15.1 and its lock contained known
advisory matches. Its build accepts unversioned PATH optimizers; executable provisioning accepts
cached files by existence and downloads executable
tarballs without a committed content digest. Its Rustls transport reaches affected certificate verification
and bundled-root dependencies. ZIP/Zstandard lock matches do not by themselves demonstrate that
those extraction paths run during Sourcefield's tarball build. The supported build no longer installs
or invokes wasm-pack.

The repository-owned adapter uses exact `wasm-bindgen-cli-support` 0.2.130 in the root workspace and
lockfile. Unlike the complete CLI package, this library does not add its HTTP/server tool dependencies.
Its internal API requires explicit `typescript(true)`, `omit_default_module_path(false)` and
`web(true)` to preserve the former web output and default initialization behavior.

At that check, official Binaryen version_133 was the current stable release, published September 21.
Its archives and extracted optimizer contents are pinned for all five supported native hosts. The macOS binaries
also require the sibling `lib/libbinaryen.dylib`; both files are authenticated. Linux artifacts have
no dynamic-library imports and the Windows executable imports only its recorded system libraries.
The official release was not marked immutable at that check. Version equality alone is insufficient:
committed hashes and same-version asset-change detection remain necessary.

The release contains the upstream parser fix for CVE-2025-14956: the fixing commit is an ancestor
of the version_133 tag, and the guarded parser loop is present. This is evidence for that correction,
not proof of absence of other native-code vulnerabilities. The independent Rust wasm-opt wrapper
embeds Binaryen 116 and its author reported inability to maintain it; adopting that stale snapshot
would not provide current optimizer maintenance.

The existing `-O` preset and six feature switches remain unchanged. Binaryen documents `-O` as
optimization level 2 with shrink level 1. The build bounds optimizer threads and streams verified
archives rather than retaining them in memory. No universal speed or memory improvement is claimed.
Cargo advisory checks cover the selected Rust graph, not the C++ optimizer or compiler supply chain.

- Binding adapter API: https://github.com/wasm-bindgen/wasm-bindgen/blob/0.2.130/crates/cli-support/src/lib.rs
- Official CLI defaults: https://github.com/wasm-bindgen/wasm-bindgen/blob/0.2.130/crates/cli/src/wasm_bindgen.rs
- Support crate metadata: https://crates.io/api/v1/crates/wasm-bindgen-cli-support/0.2.130
- Binaryen release and digests: https://api.github.com/repos/WebAssembly/binaryen/releases/tags/version_133
- Binaryen optimization options: https://github.com/WebAssembly/binaryen/blob/version_133/src/tools/optimization-options.h
- Binaryen thread controls: https://github.com/WebAssembly/binaryen/blob/version_133/src/support/threads.cpp
- Parser correction: https://github.com/WebAssembly/binaryen/commit/4f52bff8c4075b5630422f902dd92a0af2c9f398
- CVE primary record: https://github.com/CVEProject/cvelistV5/blob/main/cves/2025/14xxx/CVE-2025-14956.json
- Wrapper maintenance statement: https://github.com/WebAssembly/binaryen/issues/7552
- Cargo locked/offline builds and explicit targets: https://doc.rust-lang.org/cargo/commands/cargo-build.html
- GitHub release asset digests: https://docs.github.com/en/rest/releases/assets#get-a-release-asset

## Ubuntu browser sandbox

- Actual failing runner image permission setup (20260927.320.1): https://github.com/actions/runner-images/blob/ubuntu24/20260927.320/images/ubuntu/scripts/build/configure-system.sh
- Playwright public bundled executable path: https://playwright.dev/docs/api/class-browsertype#browser-type-executable-path
- Full Chromium headless selection and `--no-shell`: https://playwright.dev/docs/browsers#chromium-new-headless-mode
- Chromium namespace allowlisting: https://chromium.googlesource.com/chromium/src/+/main/docs/security/apparmor-userns-restrictions.md
- Legacy SUID sandbox limitations: https://chromium.googlesource.com/chromium/src/+/main/docs/linux/suid_sandbox_development.md

The image's final permission setup applies `chmod -R 777 /opt`, so its preinstalled Chrome helper
does not retain trustworthy setuid permissions. The workflow uses the documented `userns` profile
for one exact bundled executable instead. The browser path is resolved once and reused for the
policy and explicit browser launch, retaining the pinned Playwright/Chromium pairing.

## Review corrections: API contracts checked on 2026-10-06

- Vec capacity is at least the requested amount; the allocator may provide more:
  https://doc.rust-lang.org/std/vec/struct.Vec.html#method.with_capacity
- Reading to EOF does not promise a fixed final capacity:
  https://doc.rust-lang.org/std/io/trait.Read.html#method.read_to_end
- Job execution timeout and environment dispatch:
  https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idtimeout-minutes
  https://docs.github.com/en/actions/concepts/workflows-and-actions/deployment-environments
- crates.io excludes yanked versions when calculating the highest stable release:
  https://github.com/rust-lang/crates.io/blob/0ea9b2cc5237d037b7e690d26015bc458cf14d28/crates/crates_io_database/src/models/krate.rs#L193
  https://doc.rust-lang.org/cargo/commands/cargo-yank.html

The official npm metadata for both packages in the current browser lockfile was checked against
their exact resolved URLs and integrity values. Both matched; no package version changed.
At that historical check, the official full wasm-pack inventory contained the existing non-yanked
pin and matched the reported latest stable release. The former checker rejected automatic downgrades;
the current Binaryen release checker retains that protection.

## Icon catalog contracts checked on 2026-10-06

- Strict fields and tagged variants: https://serde.rs/container-attrs.html and https://serde.rs/enum-representations.html
- SVG path semantics: https://www.w3.org/TR/SVG2/paths.html#PathDataBNF
- SVG geometry: https://www.w3.org/TR/SVG2/shapes.html
- Event attributes execute code: https://www.w3.org/TR/SVG2/interact.html#EventAttributes
- SVG resource references: https://www.w3.org/TR/SVG2/linking.html
- TOML accepts nonfinite floats: https://toml.io/en/v1.1.0#float
- Explicit finite-number checks: https://doc.rust-lang.org/std/primitive.f32.html#method.is_finite
- Respecting reduced motion: https://www.w3.org/WAI/WCAG22/Techniques/css/C39.html
- Document visibility events and their bubbling behavior: https://html.spec.whatwg.org/multipage/interaction.html#page-visibility

The catalog uses typed absolute commands and semantic paint enums instead of parsing arbitrary SVG.
An element-name allowlist alone would still admit executable attributes or external references.
Geometry admission therefore happens in the shared core, before SVG rendering, and the
existing browser SVG boundary remains in place. No new dependency is required.

## Manual release contracts checked on 2026-10-06

- Dispatch captures the selected ref and commit: https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch
- Original and current initiating actors: https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#github-context
- Reruns preserve the original SHA/ref: https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs
- Draft flag, fixed target commit and workflow-file permission failures: https://docs.github.com/en/rest/releases/releases#create-a-release
- Draft lookup requires push access: https://docs.github.com/en/rest/releases/releases#list-releases
- CLI draft creation and immutable publication: https://cli.github.com/manual/gh_release_create
- Exact matching refs and empty successful responses: https://docs.github.com/en/rest/git/refs#list-matching-references
- Tag-qualified commit resolution (`tags/TAG_NAME`): https://docs.github.com/en/rest/commits/commits#get-a-commit
- Uploaded asset digests can be null: https://docs.github.com/en/rest/releases/assets#get-a-release-asset
- Immutability and non-reusable published tag names: https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases
- Repository immutability setting and administration-read requirement: https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository

The release target is ignored when its tag already exists, so both preparation and publication
require an absent tag. Preparation has read access and cannot validate every unpublished draft;
the publisher checks matching draft identity, complete asset names, sizes and digests before its
final tag check. `--draft` leaves the new tag pending until publication. A missing remote digest is
rejected rather than accepted as evidence. The publication token receives no administration scope.

## Python tooling structure checked on 2026-10-06

- Importable modules, package ownership and module search: https://docs.python.org/3.11/tutorial/modules.html
- Test discovery and module identities: https://docs.python.org/3.11/library/unittest.html#test-discovery
- Mock targets follow caller lookup: https://docs.python.org/3.11/library/unittest.mock.html#where-to-patch
- Shared dictionary type contracts: https://docs.python.org/3.11/library/typing.html#typing.TypedDict
- Remote action and reusable workflow references: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax
- Explicit test platform admission: https://docs.python.org/3.11/library/unittest.html#unittest.skipUnless
- Windows process executable search order: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw
- Windows subprocess executable selection: https://docs.python.org/3.12/library/subprocess.html#popen-constructor
- Official Windows 2025 runner shell inventory: https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md#shells

Windows can resolve its System32 WSL launcher before a Git Bash executable found through PATH.
Actual Bash dispatch fixtures are therefore scoped to POSIX; portable release CLI and wiring tests
remain enabled on Windows. This source evidence does not prove which executable a specific hosted
run started, nor does a simulated skip probe establish a full Windows test run.

The internal tooling package needs no installation or third-party dependency. Shared type hints
improve caller/IDE support but do not replace validation of release locks or consumer inputs.
Workflow-source checks enforce the current template's conventions; they are not a general YAML parser
or evidence of hosted workflow execution.

## Cross-consumer parity verification (October 7, 2026)

- [Reusable workflow inputs and secrets](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows#using-inputs-and-secrets-in-a-reusable-workflow)
  establish explicit caller input and secret forwarding; caller environment variables do not propagate.
- [Context availability](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#context-availability)
  permits caller inputs and repository variables in `jobs.<job_id>.with`.
- [Workflow identity in the job context](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#example-usage-of-job-context-workflow-identity)
  provides the reusable workflow repository and source commit used for authenticated installation.
- [Optional secrets](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets#using-secrets-in-a-workflow)
  are empty strings when unset. Warning and public-only continuation are the consumer compatibility contract.
- Official GitHub release/tag APIs verified checkout v7.0.1, upload-artifact v7.0.2,
  download-artifact v8.0.2, upload-pages-artifact v5.0.0 and deploy-pages v5.0.1 pins.
- The official Rust stable manifest and both Playwright npm package records were checked against
  the committed toolchain and lockfile. These dated checks do not claim future freshness.

## Release documentation checked on 2026-11-01 (Europe/Berlin)

The official published `v0.1.2` release was stable and immutable at this check. Its target and resolved
tag commit both identified `cd2e2b6779c82a3f64346a47d40da4967fcc0dc5`. This is a dated verification
identity, not a moving download default or a claim that the workspace's 0.1.3 build is published.

Release-lock acquisition verifies the selected release, the downloaded lock asset and its attestation
against an independently reviewed source commit. `check_pin.py` separately validates the lock schema
and literal reusable-workflow mirror. It does not authenticate a downloaded lock. The publisher uses
GitHub's generated release notes; it does not import or rewrite the repository changelog.

- Published release metadata: https://api.github.com/repos/kdominic89/sourcefield/releases/tags/v0.1.2
- Resolved release source: https://api.github.com/repos/kdominic89/sourcefield/commits/v0.1.2
- Release verification: https://cli.github.com/manual/gh_release_verify
- Exact asset download: https://cli.github.com/manual/gh_release_download
- Release-asset verification: https://cli.github.com/manual/gh_release_verify-asset
- Attestation source/signer constraints: https://cli.github.com/manual/gh_attestation_verify
- Generated release notes: https://cli.github.com/manual/gh_release_create
