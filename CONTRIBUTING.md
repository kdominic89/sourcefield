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
under Unreleased for user-visible changes; do not invent a release date or version tag.
Community participation follows the [Code of Conduct](CODE_OF_CONDUCT.md).
Use [SECURITY.md](SECURITY.md) for vulnerabilities rather than public issues or pull requests.

## Tool maintenance

Dependabot proposes Cargo, Rust toolchain, GitHub Actions and browser npm updates weekly.
`tools/browser/package.json` and its lock pin the existing Playwright tool; install it with
`npm ci --prefix tools/browser --include=dev --ignore-scripts --no-audit --no-fund`.

`tools/wasm-pack-version.txt` is the sole wasm-pack version source. The read-only weekly freshness
workflow checks crates.io and fails if a reviewed update is needed. Run
`python3 scripts/tool_versions.py --check` to check locally, or `--update` to prepare the pin change.
Review the diff, install the resulting version, rebuild WASM and run verification before proposing
its PR. A registry response never installs a tool or merges a change automatically.
