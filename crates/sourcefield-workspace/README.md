# Workspace publication and migration

This crate supplies filesystem boundaries shared by every Sourcefield consumer.
It has no network access and introduces no dependency beyond the workspace set.

## Publication contract

1. Create the consumer root and call `Transaction::begin`.
2. Stage every generated output. `stage_reader` streams large files with a 64 KiB
   buffer. Initial adoption of an existing file requires `stage_checked` with its
   observed SHA-256 digest; `None` explicitly requires absence.
3. Validate semantic output against `candidate_dir()`. The directory is read-only
   to validators: mutation invalidates staged digests. Any domain validation error
   must abort without calling `commit`.
4. Call `commit`. The crate rechecks source digests, creates synced recovery copies,
   publishes a recovery journal, installs files and records completion.

Missing owned generated files may be regenerated with absence recorded as their precondition.
A missing stale output is retired from ownership without attempting to delete an absent file.
A concurrently appearing file fails precondition validation. Omitting a previously owned file intentionally removes it. Only paths recorded in
`.sourcefield-owned.json` may be removed, and a modified owned file blocks cleanup.
Authored files outside the ownership inventory remain untouched. Managed READMEs
use `stage_authored`, which checks source preconditions and journals changes without
granting deletion ownership. Omission in a later run leaves authored files intact.

The lock uses exclusive file creation, not timing or PID-based stale detection.
`recover` and new transactions cannot bypass an existing lock. Following a process
crash, an operator must first confirm that the process has stopped, then provide
its exact lock token to `release_abandoned_lock`, followed by `recover`. A PID alone
is insufficient because operating systems reuse PIDs. Never unlock a live process.
Normal error returns and drops release the lock automatically.

File replacement is recoverable, not globally atomic. Runtime consumers can see
intermediate files; publishing jobs must deploy the validated complete candidate,
not expose a directory while promotion runs. Recovery validates every backup before
restoring anything and keeps a damaged journal for investigation. A completion
marker makes cleanup after successful publication distinguishable from rollback.

Every path is relative, UTF-8, and checked for traversal, reserved generator/Git
paths and symlinks. This is a cooperative local workspace boundary: a process with
permission to replace directories concurrently can race portable filesystem APIs.
An attacker-writable destination requires OS-specific handle-relative sandboxing,
which is outside this API contract. Root and all ancestors must be real directories, except
verified root-owned macOS `/tmp` and `/var` aliases to their expected `/private` destinations.
Admission checks the original ancestry before canonicalizing it; arbitrary symlinks remain rejected.
Windows file contents are synced; portable Rust does not provide directory handle
flushing there, so power-loss directory durability depends on the filesystem.

## One-time migration

`migration::migrate_files(source, destination, paths, variant)` receives an explicit
inventory of files/directories. Include the complete old browser/runtime, configs,
current state, observed snapshots, history indexes and all referenced archives.
The destination must be new and outside the source. It contains:

- `output/`: converted files with the same relative paths.
- `recovery/`: exact original bytes, including uncommitted files.
- `migration-report.json`: written last, with sorted source/output SHA-256 digests
  and every applied mapping. Its absence means the candidate is incomplete.

| Source | Unified output | Preservation rule |
| --- | --- | --- |
| Graph `schema = 2` | `schema_version = 3` | Nodes, edges, semantic hash and timestamps retained; unknown fields are preserved during conversion, then rejected if the current typed runtime cannot consume them |
| Missing profile variant | Explicit command variant | No inference from current observations |
| Org maintainer username/role | Maintainer object | Same identity/role; GitHub URL derived from recorded validated handle |
| Unversioned history index | `schema_version = 3` | References, counts, hashes and timestamps unchanged and checked |
| Config `version = 1` | `schema_version = 1` | All TOML values preserved apart from explicit field mappings |
| Known legacy package labels | Explicit label configuration | Records the previous renderer rules |
| Observed snapshots | `schema_version = 1` | No fresh observations or fabricated historical values |
| Runtime/assets | Exact original bytes | Matching recovery copies retained |

Already migrated documents remain byte-identical. Unsupported or ambiguous versions,
invalid handles, malformed documents, dangling history references and mismatched
hash/timestamp/count references fail. Converted graphs pass the same typed model and
semantic validator as normal runtime states before a success report is written. Repeated conversions of the same input into distinct
empty destinations produce identical outputs/reports. Recovery copies always retain
original formatting; converted TOML formatting/comments are not retained in output.
Each JSON/TOML document is limited to 32 MiB; binary copying/hashing uses a fixed-size
buffer and only one document is deserialized at a time. Metadata indexes are bounded
to 8 MiB during transaction reads and writes.

The migration tool does not activate the result. The CLI must build the new matching
runtime into the validated candidate and publish it with the converted data. Rollback
uses the entire recovery tree, including the old runtime, not only an older executable.
