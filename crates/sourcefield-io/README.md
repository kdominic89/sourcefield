# Native I/O foundation

This crate shares byte acquisition and SHA-256 mechanics between the CLI and
workspace publisher. It is not part of the browser/WASM dependency path.

Callers retain path admission, symlink checks, regular-file requirements, maximum
sizes, provenance validation, diagnostics, locking, and durability. These helpers
accept already opened streams and never authorize a path or publish a file.

| Operation | Actual consumers | Allocation behavior |
| --- | --- | --- |
| Bounded opened-file reads | CLI generation input and runtime bundles | Metadata admitted against the caller limit before allocating; one extra byte detects growth |
| Bounded UTF-8 reads | CLI local imports and legacy migration documents | Byte buffer becomes the string without a second payload allocation |
| Bounded streaming JSON | Workspace ownership and recovery metadata | Buffered parser avoids retaining a second complete serialized document |
| Byte hashing | CLI imports and runtime provenance | Unchanged lowercase SHA-256 protocol |
| Stream hashing and copying | Workspace preconditions, migration copies, transaction candidates/backups | Fixed 64 KiB scratch buffer; callers own syncing |

The generic reader shares the same overflow check as the admitted-file reader.
It deliberately does not trust arbitrary size hints for up-front allocations.
The JSON helper rejects malformed documents, trailing documents, and oversized
trailing whitespace. UTF-8 decoding rejects invalid bytes without replacement.

Focused verification:

```sh
cargo test -p sourcefield-io -p sourcefield-workspace
cargo test -p sourcefield-cli runtime::tests
cargo clippy -p sourcefield-io -p sourcefield-workspace -p sourcefield-cli --all-targets -- -D warnings
```

Allocation measurements for runtime reads at 64 KiB and the 16 MiB admission limit
retain the previous single payload allocation and exact payload capacity. This
supports preserving memory behavior; it does not establish a throughput speedup.
