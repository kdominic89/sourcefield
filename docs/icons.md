# Project icons

Select a built-in motif on a project without changing its palette or ring behavior:

```toml
visual = "migrations"
icon = "builtin:database-safe"
```

`builtin:sourcefield` draws the five-node network with three staggered signals.
`builtin:database-safe` draws the open safe and three-tier database cylinder. These use the same
typed primitives as consumer-authored icons. The `visual` field still selects the existing colors
and provides the legacy glyph when `icon` is absent. An explicit unknown icon is an error.

## Add your own motif

Copy [the complete catalog example](../examples/icons.toml) into the top level of your profile or
canonical organization manifest, then add `icon = "orbit"` to a project. No generator rebuild or
new Rust branch is needed. Definitions are inline authored data; there is no extra file loader.
TOML tables remain open until the next table heading, so keep catalog tables separate from project
fields. A minimal catalog is:

```toml
[icons.rings]
radius = 28.0

[[icons.rings.elements]]
geometry = { shape = "circle", center = [0.0, 0.0], radius = 12.0 }
fill = "surface"
stroke = "accent"
stroke_width = 1.5
```

Reference `rings` from any number of projects in that configuration. State stores the definition
once and each node stores its key. Rendering borrows it and writes primitives directly.
The authored element order is the SVG paint order; later elements cover earlier elements.

Local icon IDs start with an ASCII lowercase letter and contain only lowercase letters, digits and
hyphens, up to 64 characters. Organization namespaces keep the existing organization-ID rule:
nonempty ASCII letters, digits, hyphens and underscores, with no leading-letter restriction. They
are case-sensitive and are not restricted by the local icon's 64-character limit. For example,
`Example_Labs/rings` and `2-labs/rings` are valid qualified keys; `Example_Labs/Rings` is not.

An organization declares local icon IDs. Importing `sample-labs` qualifies `rings` and its project
references as `sample-labs/rings`. Another organization can declare its own `rings`. A consumer
project may also reference `sample-labs/rings` when that organization is explicitly imported; its
definition is resolved and checked during composition. Collisions and references to absent
definitions are rejected. Built-in references remain unchanged; consumer data cannot replace a built-in.

[Organization extraction](operations.md#organization-extraction) changes only the selected
organization's source. Other organizations remain imports, so their canonical updates keep flowing
into the consumer. Extracting an already imported organization preserves its complete canonical
manifest and existing qualified icon references. For an inline organization, extraction copies the
required definitions into its new manifest and retains definitions still used by personal projects.
If an inline project borrows a motif from another namespace, its exported manifest gets a copy under
a deterministic local alias. That copied motif is independent of the foreign manifest; the remaining
consumer's foreign import and its live references remain intact. Remote inputs require matching
captures, and `--assets` selects their directory. A configuration that both declares and imports the
same organization is rejected.

## Geometry and paint

Coordinates are absolute, centered on `[0, 0]`, with positive Y pointing down. Every definition has
an authored `radius` in `(0, 64]`; all coordinates must be finite and within that square. The renderer
clips motif content to the circular node interior. It scales down when the interior is smaller than
the authored radius and never enlarges the artwork. The built-in network uses radius 31 and the
safe uses radius 35, preserving their approved proportions at project radii 47 and 51.

Each element has a required `geometry` table and these optional style fields:

| Field | Values | Default |
| --- | --- | --- |
| `fill`, `stroke` | `none`, `accent`, `surface`, `recess`, `mint`, `purple`, `amber`, `blue` | `none`, `accent` |
| `stroke_width` | Finite number in `(0, 8]` | `1.2` |
| `opacity` | Finite number in `[0, 1]` | `1.0` |
| `line_cap` | `butt`, `round`, `square` | `round` |
| `line_join` | `miter`, `round`, `bevel` | `round` |
| `motion` | `{ kind = "signal", phase = 0 }`, phase `0`, `1` or `2` | None |

Colors resolve through the active light/dark palette. `accent` follows the project's existing
`visual`; `recess` supplies the inset safe surface. Arbitrary CSS colors and resource URLs are not
accepted. A `none` stroke does not require setting its width to zero.

| `geometry.shape` | Fields |
| --- | --- |
| `circle` | `center = [x, y]`, positive `radius` |
| `ellipse` | `center = [x, y]`, positive `radii = [rx, ry]` |
| `rect` | `origin = [x, y]`, positive `size = [width, height]`, optional nonnegative `corner_radius` |
| `path` | Nonempty `commands` array, starting with `move` |

Rounded rectangle corners may not exceed half either dimension. Path commands use a `command`
tag and explicit numeric fields:

| Command | Fields |
| --- | --- |
| `move`, `line` | `to = [x, y]` |
| `horizontal` | `x` |
| `vertical` | `y` |
| `quadratic` | `control = [x, y]`, `to = [x, y]` |
| `cubic` | `control1 = [x, y]`, `control2 = [x, y]`, `to = [x, y]` |
| `arc` | Positive `radii = [rx, ry]` up to twice the authored radius, `rotation` in `[-360, 360]`, boolean `large_arc`, boolean `sweep`, `to = [x, y]` |
| `close` | None |

There are no implicit or relative commands. Each contour must contain a drawing command; a new
`move` starts another contour and is required after `close`. SVG/XML strings,
free-form attributes, external assets, event handlers, CSS and recursive groups are not part of
this format. Strict deserialization rejects unknown fields. Validation rejects nonfinite TOML
numbers, invalid dimensions, unsupported motion, unresolved references and excessive catalogs.

Limits apply before rendering: 128 custom definitions, 64 elements per icon, 256 commands per path,
2,048 elements and 16,384 commands across the custom catalog. Imported catalogs share these aggregate
budgets. Repeated references, including built-ins, may expand to at most 8,192 elements and
32,768 path commands across selected projects. This separate usage budget bounds the SVG output
even when many nodes reuse one complex definition. The two fixed built-ins cannot increase with
consumer input.

## Motion and compatibility

The signal cycle lasts 5.4 seconds: opacity rises from 0.35 to 1 at 45% and returns to 0.35.
Phases start at 0, -1.8 and -3.6 seconds. Motion is decorative; geometry and text remain stationary.
An animated signal uses the preset opacity range; its element opacity applies when motion is
disabled. Static SVGs omit animation rules. Reduced motion disables the effects; the browser's pause and
resume controls apply to icon signals alongside the existing rings.

Config schema 1 and state schema 3 gain optional fields. Existing inputs without a catalog or
selector retain their semantic identity and SVG output. A consumer must upgrade its pinned
generator before using populated new fields; older strict readers cannot interpret them.
The complete catalog, including definitions that no project currently references, participates in
the semantic hash and is retained in `profile-state.json`. Adding, removing or editing an unused
definition therefore creates a new semantic state. This keeps identity and replay tied to the full
validated configuration. Rendering selects motifs through project references; keeping a definition
in state does not select it for a node. Catalog limits and validation also include unused entries.

Replay still requires the recorded generator fingerprint and captured inputs; it never silently
substitutes another icon definition.

To render the complete example using the current generator:

```sh
cargo run --locked -p sourcefield-cli --example icon_catalog -- --output /absolute/new-icon-preview
```

The example writes dark, light and static SVGs plus validated configuration/state. It does not
modify a consumer repository or collect network data. Its tests keep the authored TOML example
executable as the contract evolves.
