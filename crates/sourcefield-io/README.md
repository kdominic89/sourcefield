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

The portable file-read tests check exact content, empty input, size admission,
shrinkage, and growth both within and beyond the byte limit. They do not require
an exact vector capacity: Rust permits `Vec::with_capacity` to reserve more than
requested, and `Read::read_to_end` can grow the allocation.

Separate active allocation-budget tests enforce a product memory target on the
repository's pinned Rust 1.99.0 toolchain: a successful 64 KiB or 16 MiB file read
may retain at most the payload size plus 4 KiB of vector capacity. The 4 KiB margin
allows excess reservation without accepting a second payload-sized reservation.
This is a supported-toolchain performance budget, not a standard-library guarantee;
a failure requires inspecting the allocation change. These tests run in the normal
native test suite. To print the measurements explicitly:

```sh
cargo test -p sourcefield-io allocation_budget -- --nocapture
```

The measurement covers only the returned byte vector's retained capacity. It does
not measure allocator call count, transient allocations, peak RSS, or throughput.
