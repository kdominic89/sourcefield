# Changelog

User-facing changes are recorded here before release. An Unreleased entry does not mean that a
version tag, downloadable artifact or hosted deployment exists.

## Unreleased

### Added

- Built-in Sourcefield network and open database-safe motifs, with a typed consumer-owned icon
  catalog shared through organization manifests, state, replay and native/browser rendering.
  Organization namespaces follow the existing ID rules; motion respects pause, reduced motion
  and document visibility changes.

- Shared Rust generator for personal and organization GitHub profiles, animated SVGs and an
  interactive browser view with a WASM simulation and JavaScript fallback.
- Canonical organization manifests, explicit imports and stable project identities for multiple
  organizations, with managed README project and NuGet package sections.
- GitHub and NuGet metadata collection, bounded package discovery, scoped private aggregates,
  captured observations, offline replay and retained history.
- Transactional output promotion, recovery and migration tools for existing profile consumers.
- Release packaging and verification tooling for five native targets and a matching browser bundle.
- Synthetic examples, local preview instructions, contributor guidance and community templates.

### Correctness and reliability

- Give shared Python artifact, release, consumer-path and presentation checks one internal owner;
  retain existing command paths, wire formats and bounded hashing/extraction behavior.
- Align optional consumer workflow validation with the pinned reusable template and separate it from
  generated artifact admission. Split tooling tests by responsibility, reject unknown fake transport
  commands, and include nested Python packages in verification and source archives. Keep actual Bash
  dispatch checks POSIX-only while retaining portable release checks on Windows; report missing
  required consumer workflow files explicitly.

- Start releases with one owner-authorized manual dispatch on main; create version tags only after
  complete builds, provenance verification and uploaded asset checks, without a second approval.
- Set the first intended release version to 0.1.0 and make matching draft retries resumable while
  verifying already-published releases without modifying them. Reject incorrect dispatches and
  already-used tags before building; require an explicitly entered version matching Cargo.toml.

- Preserve non-selected canonical imports, source provenance and shared technology bindings during
  organization extraction; retain live local references after relocating the exported profile.
- Support `extract-organization --assets` for captured imports and saved layout outside `assets`.

- Strict publication explains approved source failures while withholding untrusted diagnostics.
- Fallback preserves observation provenance; preview data cannot become live package evidence.
- Default package positions share the same geometry across imports and automatic discovery.
- Import provenance, filesystem boundaries and release integrity have positive and negative tests.

- Refresh direct crate, Rust/browser tool and Action pins against dated primary release metadata.
- Keep Chromium sandboxing enabled on Ubuntu with an exact-executable AppArmor namespace policy.
- Write canonical provenance fixtures with explicit LF and test byte identity under Windows autocrlf.

- Keep wasm-pack in one authoritative pin with a bounded weekly registry check and reviewed update command.
- Cover Rust toolchain and locked Playwright npm updates through Dependabot while preserving browser sandboxing.
- Reject recreated SVG styles and inline style attributes before XML parsing, with adversarial browser regressions.
- Document enforced repository controls and the available GitHub private vulnerability reporting route.

- Preserve distinct component integration and target edges and reject duplicate relation entries.
- Verify replay provenance after recovery under the workspace lock and distinguish lock contention
  from filesystem errors.
- Keep collector version identification current and loopback HTTP support confined to tests.
- Verify documentation previews against the renderer and include community files in source archives.
- Diagnose yanked tool pins explicitly while retaining reviewed updates and downgrade protection.
- Check byte-reader correctness and a retained allocation budget without requiring exact Vec capacity.
- Bound release publication runtime and align maintenance documentation with the public repository.

Release dates and version sections will be added when an actual release is published.
