# Verification contract

`bash scripts/verify.sh` checks shell/Python/JavaScript syntax, Python negative and positive tests,
Node browser tests, rustfmt, all-target compilation, WASM compilation, native/doc tests, Clippy with
warnings denied and public/private API documentation. Build WASM first with the pinned packager when
verifying a release runtime. Tests must use synthetic content inside temporary consumer roots.

The shared byte-reader tests separately enforce content/size limits and the retained allocation
budget documented in [sourcefield-io](../crates/sourcefield-io/README.md). That budget is measured
against the pinned toolchain; it is not a claim about exact standard-library allocation behavior.

Release gates additionally build and smoke-test the executable on each supported native platform.
They require all native assets and the complete browser bundle from one commit. Local packaging
and mocked trust-command tests do not substitute for hosted attestation verification.

For browser changes, exercise the generated personal and organization sites at their intended Pages
base paths. Verify light/dark themes, reduced motion, keyboard navigation, mobile sizing, history
selection and JavaScript fallback, then repeat with the actual WASM bundle. Inspect node/label/curve
geometry at normal README width. Test an empty site destination and a runtime upgrade.

For performance work, record toolchain, OS/CPU, input sizes, warmup, sample count and measurement
method. Compare identical input captures, include growth/capacity boundaries and assess peak memory
as well as elapsed time. Avoid asserting a universal speedup from one machine. Release ZIP hashing
and extraction are bounded; adversarial archive tests exercise the configured expansion limit.

Failure probes cover bad checksums, unexpected source/workflow identity, unsafe archive paths,
missing native/browser assets, inconsistent pins and verification failures before installation.
Workspace tests cover competing writers and interrupted promotion. Import tests cover failed
required sources and provenance isolation. Migration tests cover original-file preservation and
unsupported formats. A release is complete only when these gates and consumer parity checks pass.

The shared validation workflow copies `tools/browser/package.json` and its lockfile into the
runner's temporary directory, runs `npm ci --include=dev`, and installs the pinned Playwright
Chromium build and Linux system prerequisites.
No browser tool becomes a product dependency. `scripts/release_browser_fixtures.py` generates the
synthetic personal, organization and multiple-organization consumers, then runs the real browser
runner against all three. Release jobs depend on this gate. Fixture generation is offline; initial
provisioning of Rust, WASM and browser test tools requires network access.

On Ubuntu 24.04, AppArmor restricts user namespaces for downloaded Chromium builds. The workflow
resolves the pinned browser with Playwright's public `chromium.executablePath()` API and installs a
root-owned AppArmor profile granting `userns` to that exact executable. The policy renderer rejects
nonliteral, relative, noncanonical, missing or nonexecutable paths. The browser runner receives
that same full Chromium path explicitly; it does not select the default headless shell. Only the
used browser is downloaded with `--no-shell`.

`chromiumSandbox: true` remains enabled. Loading this one profile does not disable AppArmor or
change global kernel settings. A rejected path or policy load fails before browser verification.
The runner-installed Chrome helper is unsuitable: the runner image applies `chmod -R 777 /opt`,
removing the helper's setuid bit and making it writable. Local Linux sandbox tests and AppArmor
parser checks complement, but cannot replace, a hosted run with Ubuntu's namespace restrictions.

Canonical manifest fixtures explicitly use UTF-8 and LF. Git's committed blob is compared with the
actual manifest bytes even when the test forces `core.autocrlf=true`. This keeps the Windows fixture
portable without weakening the production provenance check.

The caller template keeps Pages deployment in a separate job after publication. Re-running failed
deployment jobs reuses the existing Pages artifact and does not rerun the stale-HEAD publication
check or collect new data. Keep the run's artifacts until deployment succeeds.

## Artifact and preview commands

After generation, run the native `sourcefield validate --root CONSUMER` with the same configuration
path used for generation, followed by `python3 scripts/validate_artifact.py --root CONSUMER`.
For nondefault configuration/README destinations pass `--config` and `--readme` explicitly.
The supplemental standard-library validator uses `assets/resolved-config.json` for merged imported
package authority. It checks SVG safety in presentation contexts, accessible text, paired state,
README links and local resources; it does not authenticate remote imports or replace native validation.
The browser fixture gate runs both validators and geometry checks on actual generated artifacts.

For local image review with an already installed Chrome/Chromium:

```sh
python3 scripts/capture_previews.py --site CONSUMER/docs --output /absolute/new-preview-directory
```

This captures the README SVG and interactive site without installing a browser. Screenshots are
manual review evidence, not an automated assertion of visual correctness. Browser behavior remains
covered by `scripts/verify-browser.mjs` and the shared fixture gate.

`python3 scripts/verify_publication.py --binary target/debug/sourcefield` exercises two generations
and publications separated by genuine fresh clones. Git operations are confined to temporary fixture
repositories with a local bare remote. Runtime WASM/glue remains ignored while ownership is retained.
The test proves a second checkout can regenerate those missing files.

Pull requests run native Rust tests on Linux, macOS and Windows, plus portable Python distribution
checks. Linux additionally runs the complete WASM/browser gate. Hosted results are required before
claiming cross-platform execution; a local macOS run does not establish Windows success.

Dependabot covers Cargo, GitHub Actions, the Rust toolchain and the browser npm manifest/lockfile.
The separate tool-freshness workflow checks the authoritative wasm-pack pin against crates.io.
Existing `cargo clippy`, compiler/rustdoc gates,
Python AST parsing and Node syntax/tests remain required. No additional linter or audit executable
is silently introduced: dependency-advisory scanning is not claimed by these checks. A dedicated
advisory tool requires a separately reviewed tool/version and update policy.

The primary `release_browser_fixtures.py` gate always runs
`cargo test --locked -p sourcefield-cli generated_live_history_round_trips_producer_metadata` with
`SOURCEFIELD_TEST_HISTORY_OUTPUT` set to a fresh fixture directory. It forwards that path to each
browser run and requires the `real Rust producer history archive selected` result in every report.
Missing producer output or missing browser evidence fails the gate; it cannot silently skip this check.
For an individual browser investigation, export the same variable when generating the Rust fixture
and when invoking `scripts/verify-browser.mjs`.
