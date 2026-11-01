# Contributing

Use US English and ASCII in source, comments and documentation. Follow `.editorconfig`,
`rustfmt.toml` and the pinned Rust toolchain. Do not add third-party dependencies without an explicit
review of the need and maintenance cost. Do not track binaries, WASM/glue, caches or editor metadata.

Public APIs require rustdoc/docstrings. Document internal APIs when callers or IDE users benefit.
Explain a non-obvious implementation choice with a short why comment; avoid restating the code.
Keep blank lines after control-flow blocks, multiline initializations and before returns, except
for the documented compact two-line return block. Use blank lines to separate logical groups.

Each test has one Arrange, Act, Assert sequence. Separate the groups with blank lines. Split a test
that repeats that sequence into independent cases. Cover valid behavior and meaningful negative
boundaries: unsupported schemas, bad references, failed imports, unsafe paths, conflicting writers,
interrupted promotion and privacy scope. Parameterized cases must remain independent.

Run `bash scripts/verify.sh` before proposing changes. Changes to layout or browser behavior also
need actual browser verification, both themes, reduced motion, keyboard use, mobile widths and
fallback execution. Measure performance-sensitive changes with fixed inputs and report environment,
input size, latency and memory. A green test suite does not prove unchanged visual composition.

Keep changes scoped and preserve unrelated work. Commits and publication require their own reviewed
approval. Never rewrite existing consumer data as a side effect of a source test or scratch preview.

## Proposing a change

Use the bug or feature template and a minimal synthetic example before a broad design change.
Include executed checks and any compatibility impact in a pull request. Update [CHANGELOG.md](CHANGELOG.md)
for user-visible changes. Use the intended version section while preparing that release; otherwise
use Unreleased. A section groups changes, while GitHub Releases establishes publication and its date.
Do not invent a release date or imply that a version heading proves an available release.
Community participation follows the [Code of Conduct](CODE_OF_CONDUCT.md).
Use [SECURITY.md](SECURITY.md) for vulnerabilities rather than public issues or pull requests.

## Tool maintenance

Dependabot proposes Cargo, Rust toolchain, GitHub Actions and browser npm updates weekly.
Cargo minor and patch version updates share one grouped PR; major updates remain separate.
Grouping does not disable security updates or bypass review and verification.
`tools/browser/package.json` and its lock pin the existing Playwright tool; install it with
`npm ci --prefix tools/browser --include=dev --ignore-scripts --no-audit --no-fund`.

The native binding adapter in `tools/sourcefield-wasm` belongs to the root Cargo workspace and
uses its lockfile, checks and Dependabot coverage. Keep `wasm-bindgen-cli-support` exactly aligned
with `wasm-bindgen`; its internal API and generated web bindings must be checked during upgrades.

`tools/binaryen.json` owns the optimizer version, feature flags and per-host archive/tool digests.
The read-only weekly freshness workflow compares the pin with the official Binaryen release.
Run `python3 scripts/tool_versions.py --check` locally, or `--update` to prepare an explicit manifest
update. The update verifies all supported host archives before replacing the manifest. Review the
diff, rebuild with `bash scripts/build-wasm.sh` and run verification before proposing its PR.
Neither checking nor building updates committed pins automatically. Replaced assets at the same
version require investigation rather than silent adoption.

The build verifies cached executables and required libraries before each use and never selects a
`wasm-opt` or binding generator from PATH. Before execution it copies and authenticates the optimizer
and required library into a private build directory, so cache changes during compilation are rejected.
For a disconnected rebuild, fetch Cargo dependencies and complete one browser build first.
Then use `bash scripts/build-wasm.sh --offline`. Source hashes cover the producer implementation
and policy; rebuild the matching native CLI after changing them.

See [repository settings](docs/repository-settings.md) for the required main checks and the limits
of CodeQL Default Setup on external fork contributions.
