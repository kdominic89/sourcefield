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
The managed Projects table retains each approved label, summary and `display_stack`; public projects
link to their repositories and private abstractions remain unlinked. Rows follow authored domain
order and project order within each domain, including canonical manifest order. Graph normalization
and deterministic slot allocation do not reorder this presentation.

Maintainer attribution is optional. When supplied in a profile, domain, canonical manifest or state,
it requires a valid public account/link and a nonempty role after trimming whitespace. The generator
preserves the approved role text rather than inventing a missing description.

Package versions are observed data. Approved owner/family discovery can include newly published
packages automatically. Repository discovery does not automatically approve a new prominent project,
its description or access to private implementation details.

## Repository captions

Each ownership domain can opt into a compact caption below its name. The profile owns the setting
for its inline domains; a canonical organization manifest owns the setting shared by every importer.
The values are plain text, not an HTML, Markdown or interpolation template.

For an inline personal account domain, add the field to its existing `[[domains]]` entry:

```toml
repository_caption = { source = "owner-repositories", public_label = "public", private_label = "private", suffix = "repos", unavailable = "repos unavailable" }
```

For an organization, add the setting once at the top level of its canonical `organization.toml`,
before its child tables:

```toml
repository_caption = { source = "selected-projects", public_label = "public", private_label = "private", suffix = "repos", unavailable = "repos unavailable" }
```

Both personal and organization profiles use that imported organization setting. A consumer must not
copy the organization domain or maintain a second caption definition. Inline organizations support
the same field; extraction moves it into the canonical manifest and preserves other domains' settings.

| Source | Counted inventory |
| --- | --- |
| `selected-projects` | Authored projects in this exact domain with `show_in_readme = true`, grouped by `public` or `private-abstract` visibility. |
| `owner-repositories` | Available GitHub observations for the domain's configured owner, independent of the selected project circles. |

`selected-projects` keeps both counts known, including zero, and excludes discovery satellites.
For example, three public project circles and one approved private abstraction produce
`3 public / 1 private repos` in both profile variants. Hidden projects do not affect that count;
private abstractions remain unlinked. This count describes approved profile content, not access to
private organization repositories.

For personal owner totals, configure `collection.github_user` to match the account-domain owner.
The public count comes from that account observation. Its private count includes only owned private
repositories visible to the supplied token and already authorized by the existing explicit opt-in.
Credentials alone do not authorize publication. Personal private aggregates never describe an
organization or an unrelated account domain.

An organization's `owner-repositories` source uses its own available public account observation,
matched by owner; it does not substitute the aggregate across all organizations. Its private owner
count remains unavailable. Other account domains without matching configured observations remain
unavailable too. Missing values are omitted rather than replaced with zero: `12 public repos`
means that only the public count is known, while a known zero remains `0`. If both counts are
unavailable, the configured `unavailable` text is used.

Omitted caption properties use `source = "selected-projects"`, `public_label = "public"`,
`private_label = "private"`, `suffix = "repos"` and `unavailable = "repos unavailable"`.
Public/private labels must be nonempty, trimmed single-line text of at most 24 characters each.
The suffix has the same limit and may be empty to omit the trailing noun. Unavailable text must be
nonempty and at most 64 characters. Controls, line breaks, invalid XML text and unknown fields or
sources are rejected. Valid special text characters are escaped once when rendered.

An omitted `repository_caption` field preserves legacy captions, state serialization, semantic hashes
and SVG output. A supplied setting is authored semantic input, so adding or editing it changes the
state identity even when its resulting text is unchanged. Core resolves it once into optional
`Node.repository_caption`; this materialized single-line field is limited to 128 characters and is
valid only on domain nodes. Native SVG text and its accessible label consume that same string, so
the displayed and spoken counts agree. Hubs link to GitHub and do not open the project detail panel.
The stored `Node.summary` remains unchanged and supplies the legacy accessible-label fallback when no
caption is present. Visible legacy captions still count shown projects. Project descriptions, inventory,
geometry, discovery behavior and private authorization retain their existing roles.

Released `v0.1.1` rejects these new authored fields. Adopting this feature first requires a release
containing it and one consumer upgrade to its matching CLI/runtime and pinned workflow. Upgrade every
importer before publishing the new field in a shared canonical manifest: an old consumer tracking
`main` would otherwise reject the updated content. Supporting generators still accept manifests that
omit the field. After that initial adoption, label changes or switching the supported source are
configuration-only edits: rerun normal generation without a new generator release or pin change.
A live refresh fetches remote canonical content at the import's authored ref; an offline preview uses
its existing capture. An intentionally fixed commit stays fixed until its ref is updated. Authored
profile edits invalidate the previous generation record. Canonical edits become new captured inputs
only through normal generation; locked replay retains its captured version.

States and history archives containing `Node.repository_caption` cannot be read by `v0.1.1`, whose
state parser rejects unknown fields. Roll back by restoring a complete compatible profile revision,
including authored configuration, captured imports, current output, history and matched generator
pins. The older generator reads existing history before publishing and cannot convert newer archives
in place, even with `--no-history`. If a remote canonical import tracks `main`, pin it to a compatible
manifest revision before the next live refresh. Changing only the generator pin is insufficient.

## Shared technologies across ownership domains

Organization manifests retain every declared technology, including technologies connected only by an
organization affinity and references inside approved project components. A canonical technology's
`affinities` may contain its manifest's organization ID or be empty. References to another consumer's
domain are rejected; consumer-specific ownership relationships belong in the profile configuration.
A local technology may declare an affinity to an inline domain or an explicitly selected import ID.
Authored validation defers the selected imported domain until composition, which requires its actual
canonical manifest. Unselected domains are rejected. Local project and publication ownership still
requires an inline domain; this affinity rule does not authorize adding facts to canonical imports.

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
