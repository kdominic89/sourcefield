# Configuration and content lifecycle

`config/profile.toml` is consumer-owned TOML with `schema_version = 1`. The Rust `Config` and
`OrganizationManifest` types document every supported field. Unknown fields, unsupported versions,
duplicate identities, invalid references and stale explicit layout overrides are errors.
`examples/organization.toml` is synthetic canonical organization content.
Projects can select a built-in or consumer-owned `icon` independently of their existing `visual`.
See [the icon catalog](icons.md) for authoring, organization namespacing and validation limits.

A local organization import resolves its path relative to the profile configuration directory:

```toml
[[imports]]
id = "example-labs"
source = { kind = "local", path = "organization.toml" }
```

A remote import identifies one explicit public repository and authored ref:

```toml
[[imports]]
id = "example-labs"
source = { kind = "remote", repository = "example-labs/.github", ref = "main", path = "config/organization.toml" }
```

A refresh resolves the ref to one commit and fetches the manifest at that commit. The contents API's
file SHA is a blob identity, not the source commit. Captured provenance preserves repository, path,
requested ref, resolved commit and digest. Local inputs remain explicitly local; they cannot claim
remote provenance. Import order defines organization presentation order.

Stable graph identities qualify projects as `project:example-labs/database`. Manual geometry uses
these complete identities:

```toml
[layout.overrides]
"project:example-labs/database" = [420.0, 500.0]
```

Existing recorded assignments remain stable when projects are added. New projects receive free
positions in deterministic order. Additional areas and package-group rows expand the canvas.
Capacity errors require correcting content/layout; they do not authorize invisible clipping.

Add a project once in the canonical manifest. Rename its label while retaining its stable ID.
Removing a project also requires removing obsolete authored layout overrides. Generated assignments
for removed content are retired. README project/package sections, SVG and browser views derive from
the same resolved facts. Surrounding authored README prose stays outside managed markers.

Package versions are observed data. Approved owner/family discovery can include newly published
packages automatically. Repository discovery does not automatically approve a new prominent project,
its description or access to private implementation details.

## Shared technologies across ownership domains

Organization manifests retain every declared technology, including technologies connected only by an
organization affinity and references inside approved project components. A canonical technology's
`affinities` may contain its manifest's organization ID or be empty. References to another consumer's
domain are rejected; consumer-specific ownership relationships belong in the profile configuration.

A consumer can explicitly bind an imported technology to an existing local technology:

```toml
[shared_technologies]
"example-labs/rust" = "rust"
```

The key is the canonical `organization-id/local-technology-id`; the value is an existing consumer
technology ID. Both definitions must agree on `label` and `category`. Composition keeps one local
technology node, preserves its consumer affinities and presentation flag, restores any declared
canonical organization affinity, and retains either definition's `cross_domain` capability. Project,
component and package-group references all resolve to that shared node. Identical names alone never
merge technologies automatically.

Bindings with missing consumer targets, absent selected canonical sources or conflicting definitions
are errors. Updating or removing canonical technology IDs therefore requires updating their bindings.
The extraction command emits bindings for retained shared or consumer-referenced technologies and
preserves approved project coordinates, radii and weights in consumer-owned layout configuration.

`collection.history_limit` must be in `1..=256`. Retention, the archive writer and the reader share
that bound; rollover removes the oldest retained entry instead of constructing an unreadable archive.

Authored NuGet links use `https://www.nuget.org/packages/{id}` with an optional trailing slash.
The path ID must match the package ID case-insensitively. Credentials, extra path segments, query
strings, fragments and alternative hosts are rejected rather than copied into managed Markdown.
