# Generation, migration and recovery

## Explicit destinations

`sourcefield generate --root PATH` resolves consumer input/output paths below that root. Select asset
and site paths with `--assets` and `--docs`; repeat `--readme` for each intended managed README.
No wrapper infers README writes. Keep previews in a separate consumer copy when published outputs
must remain unchanged. Existing generated files require explicit one-time adoption during migration;
normal generation does not seize ownership of arbitrary files.

The selected workspace root must exist and be a directory. Relative roots resolve from the current
working directory. Custom symlinks in the root or any ancestor are rejected before transaction creation;
the error identifies the rejected component. On macOS only root-owned `/tmp` -> `/private/tmp` and
`/var` -> `/private/var` system aliases are accepted, with their exact targets checked. This exception
does not permit custom descendants. Select a direct physical directory path rather than a custom alias.

Use `--strict-live` for scheduled refresh. Use `--offline` for a deliberate cached preview and
`--offline --locked` to reproduce captured resolved configuration and observations. An empty offline
machine needs the matching CLI/runtime and captured inputs provisioned first. Missing inputs fail
with a diagnostic; they are not fabricated. The generation transaction validates the complete
candidate before promotion. `sourcefield validate --root PATH` provides an additional artifact check.

For successful live refreshes with identical observation content, Sourcefield retains the previous
capture and its original `fetched_at`. That timestamp describes the retained observation, not today's
poll. Changed values, source status, warnings or mode create a new capture. This avoids commits caused
only by the passage of time while keeping archived provenance truthful; it does not suppress real changes.

Ignored generated runtime files may be absent after a fresh checkout. Generation recreates missing
owned output while still refusing to overwrite changed owned bytes or arbitrary unowned files.
The tracked ownership inventory remains complete even though binary/glue output is not committed.

Default online generation preserves a partial collection as `Partial`, including explicit failed
sources. If the entire collection fails, permissive generation may use only a usable, dated capture;
an offline preview fixture is not a valid fallback. Missing or unusable captures fail. Scheduled
`--strict-live` generation rejects partial results, fallback results and warnings before publication.

A repeated total failure reuses the same dated fallback and adds its diagnostic only once; it does
not grow the warning list or change owned artifacts when the inputs are otherwise identical. Package
cache restoration uses the same dated, non-preview admission rule. A preview seed or an explicitly
missing/preview package observation cannot become fallback evidence during a partial collection.

Strict rejection names approved incomplete sources and recognized warning causes. For example,
reaching `repository_limit` with a full final page does not prove that repository discovery exhausted
the account, even when the actual count equals the limit. Other diagnostic text and unapproved source
identifiers are withheld to avoid printing private data or upstream payloads. Source statuses must
also be live; a contradictory `Live` snapshot containing a missing source is rejected.

Collection status is scoped to the selected account and publication groups. Repository checks carry
`github:repositories:{owner}` identities; an existing legacy aggregate repository status is retained
conservatively only when repository collection is requested for selected owners. NuGet service-index
status is retained only for selected NuGet groups, and failed authored package checks remain visible
even when no package payload was returned. Scoping never promotes `Partial` to `Live`. If filtering
removes all detailed failure reasons, a fixed `collection:partial` status preserves that uncertainty.
Organization output removes raw warning text and unselected source details to avoid disclosure.

Remote organization imports fail closed independently of observation collection. Online generation
must resolve and load every required remote import successfully, even without `--strict-live`.
A dated GitHub/NuGet observation fallback does not authorize stale imported configuration or silently
remove a failed organization. The error identifies the failed import. `--offline` uses available captured
remote imports without network access; absent required captures fail. `--offline --locked` additionally
checks the recorded authored inputs, capture digests and generator identity before replay. Neither mode
fabricates missing import content or treats a remote failure as permission to replace configuration.

## One-time migration

Use `sourcefield migrate --source OLD --destination NEW --variant personal` or `organization`.
The destination is a new migration directory containing `output/` and recovery material. The source
remains unchanged. Inventory current state,
source snapshots, history indexes and archives first. Review the report and per-file digests against
semantic facts, timestamps, identities, maintainer information and reference coverage. Already
converted inputs must remain identifiable; unknown formats are errors, not guessed conversions.

Keep the original data and matching runtime, including uncommitted files. Before promoting a migrated
consumer, verify current and historical browser views, generation after migration, retention and a
complete rollback. Normal CLI/browser readers use the current schema only; legacy conversion remains
in the migration tool. Do not substitute today's metrics for absent historical facts.

For the existing README format, migration wraps the exact legacy Projects table in managed
project markers while preserving its original text. Duplicate headings, partial markers and
intervening prose fail with a diagnostic. Already marked sections remain unchanged. Normal
generation requires explicit complete marker pairs and never guesses authored boundaries.

To extract an existing organization into canonical content, use `sourcefield extract-organization
--config PATH --organization ID --destination NEWDIR`. This emits `profile.toml` and
`organization.toml` while retaining consumer-owned positions, radii and weights. Review the extracted
facts and privacy boundary before adopting them in the owning organization repository.

## Publication and interruption

Generation does not commit, push or deploy. A caller receives a validated candidate; its publication
job checks the source revision, serializes writes and publishes only after validation. Pages must
use that same candidate. Concurrency cancellation is disabled during publication.

If a local transaction is interrupted, preserve its journal and recovery material. Confirm no
writer remains, read the exact `.sourcefield-lock` token, then run
`sourcefield recover --root PATH --abandoned-lock-token TOKEN`. A changed token is rejected.
Without a lock, run `sourcefield recover --root PATH`. Do not
manually mix candidate files with live output. Recovery restores the matching complete file set.

README commits and Pages deployments are separate. A deployment failure after a successful push is
a partial publication, not a successful update. Retry deployment of the existing candidate and check
the served identity. Keep replay inputs under version control so artifact expiration is survivable.
