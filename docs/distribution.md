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

The reusable workflow accepts `config`, `readmes` (JSON array), `caller_workflow`, `offline`,
`locked` and `include_private_count` (boolean, default `false`). Its only optional explicit secret
is `PROFILE_TOKEN`. It returns a complete validated candidate artifact and generator source commit.
It never commits or deploys. Local installation
reads the identical lock and uses the same complete runtime.

The caller template preserves the manual `include_private_count` input and the repository variable
`SOURCEFIELD_PRIVATE_COUNTS=true`. It forwards their selection through the reusable workflow's
typed boolean and passes `PROFILE_TOKEN` separately as an optional explicit secret. Caller `env`
values do not cross the reusable-workflow boundary. A selected count adds native `--private-counts`
only when the token is nonempty. Without it, candidate preparation emits a warning and continues
strict public generation. A token alone does not enable collection; only the aggregate count of
owned private repositories may be collected, never their names. Native CLI configuration and
`SOURCEFIELD_PRIVATE_COUNTS` environment opt-ins remain independent and are not overwritten by
this additional caller selection. Their existing credential and strict-mode policy still applies.

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

Candidate generation explicitly selects `assets/source-snapshot.json` for retained observations
and collection fallback. Offline updates require that capture; a missing capture fails rather than
substituting the empty authoring seed. A first online refresh can collect without a prior capture
or seed. Malformed or unsupported existing snapshot input still fails; strict collection failure
publishes no output.
The native CLI's default `config/offline-snapshot.json` remains an initial direct-authoring seed.

Offline generation and locked replay preserve an existing `assets/source-snapshot.json` byte-for-byte.
The separate `assets/render-snapshot.json` records the effective, privacy-filtered rendering input:
an offline Preview has no retrieval date, while the retained Live capture keeps its original date
and source statuses for a later refresh. Keep both snapshots and the generation record tracked;
existing approved captures are not rewritten merely because current rendering omits a private count.

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

## Manual release

The first immutable release is [`v0.1.0`](https://github.com/kdominic89/sourcefield/releases/tag/v0.1.0),
published on October 7, 2026. The workspace version describes the next release being prepared;
unreleased source does not provide an authenticated consumer release pin.
In GitHub, open **Actions -> Release Sourcefield -> Run workflow**, select `main`, enter the exact
workspace version without `v`, and start the workflow once. That owner dispatch authorizes publication;
there is no second environment review or manual tag push. The workflow also checks the owner on a
publish-only retry, because its current initiating actor can differ from the original dispatcher.
The version input has no default. A dispatch from another branch fails with a clear error.

The dispatch fixes `github.sha` for validation, every build, package, attestation and final release.
Before any target build, the version must match `Cargo.toml` and its tag must be absent. This read-only
preflight rejects an already-used version without repeating the build matrix; draft identity is
checked later with publication permissions. All five native targets and the complete browser bundle
must pass before publication. The publisher verifies checksums and build
provenance, creates a draft without a Git tag, uploads all files and checks their exact remote
inventory and SHA-256 digests. Publishing the complete draft creates the tag at that same commit.
A failed build or upload does not reserve a version tag. A failed upload can leave a resumable draft.

A rerun keeps the original commit. Resume a failed upload only against its matching draft and full
source SHA. If source corrections are needed, remove the failed unpublished draft before dispatching
the corrected main commit for that version; never retarget an existing draft silently. If publication
already succeeded but final verification failed, retrying publication only verifies the existing
release and assets. It never replaces a published version. Published tags and files remain immutable.

The `release` environment allows only `main` and has no required reviewers or wait timer.
The creation-only tag restriction is disabled so the write-scoped workflow token can create its tag;
the independent version-tag integrity rule remains in place. Owner/main checks govern this workflow,
not every possible caller with repository write access. No additional token or GitHub App is needed.

The repository's immutable-release feature must be enabled before setting
`SOURCEFIELD_IMMUTABLE_RELEASES=true`. This variable confirms the inspected setting; it does not
activate immutability. The publication script refuses to publish without the exact confirmation.
Uploaded artifacts and configured settings do not by themselves prove a successful immutable release.
Verify the actual publication and its release/asset attestations before selecting it in consumer locks.

GitHub may reject publication of an older fixed commit if `main` has changed its workflow files
while the release was building: that API case requires a permission the workflow token cannot hold.
The API returns `404 Not Found`, or `403 Resource not accessible by integration` on some authentication
paths. Such a failure remains visible and never substitutes the newer commit. Keep workflow updates separate
from an active release attempt. See GitHub's [release API permissions](https://docs.github.com/en/rest/releases/releases#create-a-release).

[Repository settings](repository-settings.md) records the contribution and release controls.

Primary behavior references: [manual dispatch](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch),
[release target and workflow permissions](https://docs.github.com/en/rest/releases/releases#create-a-release),
[CLI draft creation](https://cli.github.com/manual/gh_release_create),
[immutable release guidance](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases),
and [rerun commit identity](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs).

## Installed runtime and generated site metadata

Installation verifies the complete raw runtime against its authenticated release provenance before
rendering any consumer identity. Generation projects the validated profile identity into the HTML,
web manifest and favicon, and records those three output digests in the generated site's runtime
manifest. The verified source revision and source fingerprint remain unchanged. The installed runtime
is never modified; generated `docs/` is output and cannot be reused as the raw `--runtime` installation.
