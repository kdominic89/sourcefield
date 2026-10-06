# Maintaining Sourcefield

## Release prerequisites

Create the public `kdominic89/sourcefield` remote only after authorization. Enable immutable releases,
configure the protected `release` environment, and review who may push version tags or approve its
jobs. Set the repository variable `SOURCEFIELD_IMMUTABLE_RELEASES` to `true` after checking the setting.
The variable records operator setup; it is not API proof of the setting. The settings endpoint needs
administration-read permission, which the publication token intentionally lacks. The workflow verifies
the actual immutable release immediately after publication and reports failure if verification fails.
Do not grant repository administration to the publication token.

The publish job has a 20-minute execution timeout for artifact download, verification, upload and
publication. Environment approval happens before runner dispatch and has its own GitHub wait limit.
After a timeout, inspect the release: retry an incomplete draft, or verify an already-published
immutable release before deciding on further action. Never delete or overwrite a published release.

The repository version and intended version tag must agree. Review both real consumers with captured
inputs and a synthetic second organization before approving a release. The release workflow runs
verification, builds and smoke-tests all five native targets, packages the complete WASM/browser
runtime, attests every archive and assembles a complete draft before publication. A missing target
blocks the release. Failed uploads leave a draft. Existing published releases are never overwritten.

A workflow file is not evidence that these hosted jobs have run. Until a remote, protected environment
and approved tag exist, local verification covers packaging and trust/failure tests; hosted native
matrix execution, attestations and actual publication remain observable release gates.

## Consumer rollout

Obtain the verified release lock, prepare both consumer workflow/lock updates and inspect their diffs.
Use `scripts/check_pin.py --prepare` to create a review directory containing the matched files.
Generate both candidates with their selected input captures, inspect output and run browser checks.
Only then remove duplicate authored generator/runtime implementation from consumers.

Coordinate the two pin updates. During staggered rollout, record which consumer still uses the old
revision. Cross-repository commits and README/Pages publication are not one atomic operation.
Generator updates are deliberate; ordinary content refreshes follow each consumer's schedule.

## Recovery and retention

Preserve captured input state and history in consumer Git. Keep a recovery copy containing actual
uncommitted inputs and matching browser runtime before the one-time migration. Actions artifacts
are retained for 14 days for operational convenience, not as the only recovery source. A previous
consumer revision plus its captured inputs and verified immutable generator release reconstructs
that generation. Preserve an offline copy of releases needed for recovery if external availability
must not be assumed.

On interrupted local promotion, use `sourcefield recover --root PATH` according to the ownership
journal; establish that the original process has stopped before releasing a stale lock. Restore the
whole runtime/data/index set. Never downgrade only the executable against a newer state format.

If the generated README commit succeeds but Pages fails, retry deployment of that same candidate.
Do not collect new live observations merely to retry deployment. Verify the served version afterward.

## Dependency updates

Before an initial publication or release proposal, compare every direct crate, the pinned Rust and
WASM/browser tools, and all Action references with their latest stable primary releases. Include
consumer workflow templates in the Action audit. Checking that an existing version is published
or that a build passes does not prove freshness. Record the retrieval date, release identities and
Action commits in [SOURCES.md](SOURCES.md), then verify the updated lockfile, native workspace and
actual WASM/browser bundle. Dependabot remains enabled for subsequent updates.

## Public documentation

Before publication, verify the private contact in SECURITY.md and CODE_OF_CONDUCT.md. If GitHub
private vulnerability reporting is enabled, check its actual availability before advertising it.
Keep the README quickstart executable from a clean checkout and a new temporary directory.

The three SVGs in `docs/preview/` are intentional documentation assets generated from the synthetic
`config/profile.toml` and `config/offline-snapshot.json`. The native all-targets tests compare their
complete bytes with the current renderer, using a fixed timestamp and XML numeric character
references for non-ASCII text. To refresh all three after an intentional change, run:

```sh
cargo run --locked -p sourcefield-cli --example documentation_previews -- --write
```

Omit `--write` to check freshness without modifying the files. Review all three SVG diffs and inspect
both themes and reduced motion. Do not copy runtime WASM, captures or transaction files into the
documentation assets.

Before an actual release, review Unreleased in CHANGELOG.md and move the shipped entries to the
matching version and publication date. Keep unreleased work separate. Do not rewrite the changelog
to imply that local verification is evidence of hosted publication.
