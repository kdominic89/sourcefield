# Architecture

## Data flow

Authored TOML and explicit organization imports are validated into one shared configuration.
A refresh resolves each remote import once to a commit, collects allowed GitHub/NuGet observations
and records their provenance. A replay reads captured inputs without refreshing them. Stable layout
assignments and authored overrides feed the graph, SVG, README and browser projections.

The CLI prepares every output inside one candidate transaction. Runtime files, current state,
observations, imported facts, history and explicitly selected README changes are validated before
promotion. The workspace layer tracks generated ownership and recovery state. It does not commit,
push or deploy. Consumer automation publishes an already validated candidate.

## Crate boundaries

- `sourcefield-io`: bounded byte/JSON I/O and digest primitives shared by native consumers;
  each caller retains its own path, size and provenance policy. It is outside the WASM graph.
- `sourcefield-core`: typed schema, strict validation, privacy filtering, graph and stable placement.
  Its `registry` module owns managed README project/package projections. Its icon module owns
  the bounded primitive catalog shared by configuration, organization imports and state admission.
- `sourcefield-collector`: scoped public GitHub/NuGet collection and bounded discovery.
- `sourcefield-render`: static/animated SVG projections.
- `sourcefield-cli`: orchestration, import/replay policy and explicit filesystem destinations.
- `sourcefield-wasm`: browser simulation over numeric position/link buffers supplied by JavaScript.
  It does not depend on `sourcefield-core`; the browser validates state before adapting it to
  the numeric WASM interface. Native schema types are not compiled into the simulation.
- `sourcefield-workspace`: generated ownership, candidate promotion and interruption recovery.
- `sourcefield-workspace::migration`: isolated one-time conversion of legacy consumer data.

Browser source is authored in `runtime/`. Generated `runtime/pkg/` is a build artifact. Technical
manuals live in `docs/`; they are not a deployable profile site. Runtime files reach consumers through
the selected release or a deliberate local source build, never through sibling-path assumptions.

## Identity and trust boundaries

Organization IDs are stable namespaces; display labels and account handles are facts. Project IDs
are qualified by organization. Authored selection controls visible projects. Registry discovery is
limited to approved owners and exact roots/dot-separated descendants. An organization profile must
exclude personal observations even when its input cache was previously used by a personal profile.

Remote manifests are data, never scripts. Imported paths and schemas are checked before use.
Icons contain typed absolute geometry, semantic colors and a fixed motion vocabulary. Catalogs are
shared per configuration/state; nodes hold references. Organization-local keys and references are
qualified together. The renderer borrows definitions and emits primitive markup with renderer-owned
clipping identifiers; consumer input cannot introduce XML, CSS, scripts or external resources.
Network collection failures cannot authorize silent source removal. Live generation and historical
replay have distinct provenance. Tokens are process inputs and never persisted with observations.

## Version boundaries

CLI release versions, configuration schema versions and generated state schema versions serve
different contracts. The one-time migration converts known historic inputs into the current state
schema; normal readers reject old and unknown formats. Migration code remains independently testable.
The release lock binds CLI, browser runtime, workflow source and artifact digests to one source commit.

## Performance and memory

Native code owns collection and deterministic projections; browser simulation retains its bounded
working buffers. Streams and incremental hashing avoid reading entire release archives into memory.
ZIP extraction checks member count and total expanded size before writes. Collection/layout resource
ceilings fail with diagnostics rather than allowing unbounded work or silently truncating content.

Preserve stable assignments when content grows. Avoid recomputing layout from animation state and
avoid cloning complete observation trees per frame. Performance changes require measurements on
representative and boundary-size fixtures, including allocation-sensitive browser paths.
