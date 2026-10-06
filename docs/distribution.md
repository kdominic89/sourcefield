# Pinned distribution and consumer integration

## Release identity

`sourcefield.lock.json` has schema version 1 and exactly these fields:

- `repository`: `kdominic89/sourcefield`.
- `source_commit`: complete lowercase 40-character commit SHA.
- `release`: exact version tag, never `latest`.
- `workflow`: `.github/workflows/release.yml`.
- `assets`: each supported native target plus `browser`, each containing `name` and SHA-256 `sha256`.

The release workflow produces this lock from the actual six archives. Do not hand-invent asset
hashes, a release or an accessible source revision. All archives contain matching release metadata.
Native targets are `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`. Linux builds target the Ubuntu 24.04 GNU
runtime baseline; other libc/older glibc platforms require an independently tested source build.

## Verified installation

Use bootstrap tooling from the reviewed generator source revision, Python 3.11+ and GitHub CLI with
release/attestation verification support. Authenticate `gh` with appropriate read access:

```sh
python3 scripts/bootstrap_release.py \
  --lock /consumer/sourcefield.lock.json \
  --destination /new/installation/directory
```

The installer checks the exact release, SHA-256 digests, each release asset, and build attestations
bound to repository, signer workflow, source commit and signer commit. Both archives are verified
before extraction. Extraction rejects traversal, links, duplicate/case-aliased members and excessive
expanded size. Installation publishes a new complete directory only after both metadata records
match. Existing installations are never overwritten. A failed verification never executes the CLI.

A new machine needs network access for initial installation. Preserve an installed complete pair
for offline replay; do not download or execute a moving installation script. Build provenance proves
origin, not that the source has no defects.

## Workflow pin and local agreement

`.github/workflows/generate.yml` is reusable and read-only. The caller uses its complete SHA as a
literal reference. `sourcefield.lock.json` records the same SHA. The workflow checks both against
`job.workflow_sha`, which identifies the reusable workflow's own source rather than the caller.
This identity API is a GitHub.com requirement; Enterprise Server compatibility is not claimed.

The reusable workflow accepts `config`, `readmes` (JSON array), `caller_workflow`, `offline` and
`locked`. Its only optional explicit secret is `PROFILE_TOKEN`. It returns a complete validated
candidate artifact and generator source commit. It never commits or deploys. Local installation
reads the identical lock and uses the same complete runtime.

For a reviewed upgrade, obtain the new authenticated lock and prepare the paired change:

```sh
python3 scripts/check_pin.py --lock /download/sourcefield.lock.json \
  --workflow /consumer/.github/workflows/update-profile.yml --prepare /new/review-directory
```

Inspect and adopt both files together after both consumer previews pass. The preparation command
changes neither the active workflow nor Git. CI rejects a mismatched literal reference.

## Caller publication

`docs/consumer-workflow.yml.template` is deliberately inert. Replace its generator SHA only with an
existing reviewed release's source SHA and add its verified lock. The consumer owns publication
permissions, schedule and environment. For organization profiles, select both `README.md` and
`profile/README.md`. Profiles share no write token. Do not use `secrets: inherit`.

Publication is serialized per consumer and validates the expected source HEAD before pushing.
The manifest separates generated `files` from explicitly staged `authored_files` (managed READMEs).
Only generated ownership authorizes cleanup; omission of a README never authorizes its deletion.
Compiled `docs/pkg/` stays ignored by Git while the Pages artifact includes it. Keep the retained
input capture and generated ownership manifest tracked. Deployment follows the generated commit;
a push conflict stops deployment rather than publishing a candidate from obsolete inputs.

Candidate generation preserves an independent shallow Git checkout during first-party canonical
import verification. The original GitHub origin, exact source HEAD and committed manifest bytes
remain available; dirty canonical content is not relabeled as committed. The private Git metadata
is removed even on generation failure and never enters the uploaded candidate artifact.

The caller uploads the Pages artifact before committing generated output. An upload failure therefore
leaves the expected Git revision unchanged and publication can be retried safely. The separate deploy
job depends on successful publication; an uploaded artifact alone never authorizes deployment.
Each publication attempt uses a distinct Pages artifact name. The completed publication job exposes
that exact name as an output; deployment retries consume the saved output instead of reconstructing
a name from their newer run attempt.

Candidate preparation validates every tracked repository pathname before cloning or creating the
candidate directory, including files outside the selected configuration and README destinations.
All names must use normalized relative forward-slash paths. Colons and backslashes are rejected on
all hosts to preserve the same contract on Windows and POSIX; control characters, absolute paths,
parent traversal and case-insensitive `.git` components are also rejected. The diagnostic includes
the offending name and the reason. Rename a nonportable tracked file explicitly in the consumer;
Sourcefield never silently omits it from the candidate or rewrites its name.

Required remote organization imports are an input trust boundary, not optional observation data.
A failed online import prevents candidate publication even when observation fallback is permitted.
Provision complete captured imports for offline generation; locked offline replay additionally enforces
recorded input and generator identity. An observation cache cannot replace missing imported configuration.

## Publisher approval

The `release` environment requires approval by the repository owner. Self-review is permitted
because the owner is currently the only maintainer. Disable administrator bypass through the
GitHub environment UI before publication. Its deployment policy accepts version-tag refs matching `v*`; the workflow
itself starts only for `v[0-9]*` tags. Tag creation is owner-restricted independently from rules that
prevent existing version tags from being updated or deleted. A tag pattern alone does not prove
that its commit belongs to main: select an already merged, verified main commit for publication.

The repository's immutable-release feature must be enabled before setting
`SOURCEFIELD_IMMUTABLE_RELEASES=true`. This variable confirms the inspected setting; it does not
activate immutability. The publication script refuses to publish without the exact confirmation.
Approval, uploaded artifacts and configured settings do not by themselves prove a successful
immutable release. Verify a separately authorized publication and its release/asset attestations
before selecting it in consumer locks.

[Repository settings](repository-settings.md) records the contribution and release controls.
