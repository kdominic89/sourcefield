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
WASM tool versions are pinned in `rust-toolchain.toml` and `tools/wasm-pack-version.txt`.
The current Playwright pin and dependency closure live in `tools/browser/package.json` and
`tools/browser/package-lock.json`; dated registry observations below are historical evidence.

- Playwright CI/browser provisioning: https://playwright.dev/docs/ci
- Exact existing browser test tool metadata: https://registry.npmjs.org/playwright/1.63.0
  (primary registry returned version 1.63.0 and exact playwright-core dependency 1.63.0; checked 2026-10-04).

- Dependabot ecosystems, weekly schedules and grouped Actions updates: https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference

## Dependency freshness snapshot

Before choosing or refreshing pins, inspect the latest non-yanked stable crate releases, the stable
Rust distribution manifest, and the official tool/Action releases. Record the retrieval date and
resolved Action commit; a successful build alone does not establish dependency freshness. Recheck
these sources before proposing publication. Future releases can make this dated snapshot obsolete.

Verified on 2026-10-04:

| Direct crate | Exact pin | Primary release metadata |
| --- | --- | --- |
| anyhow | 1.0.104 | https://crates.io/api/v1/crates/anyhow |
| chrono | 0.4.45 | https://crates.io/api/v1/crates/chrono |
| clap | 4.6.7 | https://crates.io/api/v1/crates/clap |
| hex | 0.4.3 | https://crates.io/api/v1/crates/hex |
| reqwest | 0.13.5 | https://crates.io/api/v1/crates/reqwest |
| serde | 1.0.229 | https://crates.io/api/v1/crates/serde |
| serde_json | 1.0.151 | https://crates.io/api/v1/crates/serde_json |
| sha2 | 0.11.0 | https://crates.io/api/v1/crates/sha2 |
| thiserror | 2.0.21 | https://crates.io/api/v1/crates/thiserror |
| tokio | 1.53.2 | https://crates.io/api/v1/crates/tokio |
| toml | 1.1.6+spec-1.1.0 | https://crates.io/api/v1/crates/toml |
| wasm-bindgen | 0.2.129 | https://crates.io/api/v1/crates/wasm-bindgen |

Cargo's TOML requirement uses `=1.1.6` because build metadata is ignored in version requirements;
the lockfile retains the complete published version. Reqwest 0.13 moves query encoding behind the
`query` feature and selects Rustls with `rustls`; only `json`, `query` and `rustls` are enabled here.
The Rustls backend now uses reqwest's default AWS-LC provider and platform certificate verifier.
Its transitive libraries are resolved in `Cargo.lock`; no separate direct TLS dependency is added.

- Reqwest 0.13 changes: https://github.com/seanmonstar/reqwest/releases/tag/v0.13.0
- Rust 1.99.0 stable distribution: https://static.rust-lang.org/dist/channel-rust-stable.toml
- wasm-pack 0.15.0: https://crates.io/api/v1/crates/wasm-pack
- actions/checkout v7.0.1: https://github.com/actions/checkout/releases/tag/v7.0.1
- actions/upload-artifact v7.0.1: https://github.com/actions/upload-artifact/releases/tag/v7.0.1
- actions/download-artifact v8.0.1: https://github.com/actions/download-artifact/releases/tag/v8.0.1
- actions/attest v4.2.2: https://github.com/actions/attest/releases/tag/v4.2.2
- Direct attestation action recommendation: https://github.com/actions/attest-build-provenance/blob/v4.2.2/README.md
- actions/upload-pages-artifact v5.0.0: https://github.com/actions/upload-pages-artifact/releases/tag/v5.0.0
- actions/deploy-pages v5.0.1: https://github.com/actions/deploy-pages/releases/tag/v5.0.1
- Ubuntu runner-installed browsers: https://github.com/actions/runner-images/blob/main/images/ubuntu/Ubuntu2404-Readme.md
- Python explicit text-file newlines: https://docs.python.org/3/library/pathlib.html#pathlib.Path.write_text
- Git text normalization: https://git-scm.com/docs/gitattributes

- Rustls platform certificate-store behavior: https://github.com/rustls/rustls-platform-verifier/blob/main/README.md
- AWS-LC native build requirements: https://aws.github.io/aws-lc-rs/requirements/index.html

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
The official full wasm-pack inventory contained the existing non-yanked pin and matched the
reported latest stable release. The tool continues to reject automatic downgrades.

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
