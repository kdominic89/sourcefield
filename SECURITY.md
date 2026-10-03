# Security policy

## Reporting a vulnerability

A private reporting channel is not currently available. Please do not post sensitive information
in public issues.

Once a private channel is published here, include the affected version or commit, minimal reproduction
steps, prerequisites and expected impact. Use synthetic inputs and redact tokens, private captures
and personal data. Coordinate disclosure with the maintainer.

GitHub private vulnerability reporting is not assumed to be enabled. No response time is promised.

## Versions and fixes

The current development line is the starting point for investigation and fixes. Include the exact
release or source commit you use, even if it is older. Published versions, when available, are listed
in CHANGELOG.md and GitHub Releases. No separate long-term support or backport schedule is promised.

## Scope and trust boundaries

Sourcefield includes a native CLI, GitHub/NuGet collectors, configuration and import processing,
filesystem generation and recovery, release/bootstrap tooling, and a static browser/WASM runtime.
It processes authored configuration, remote metadata, captured observations and release archives.
These inputs cross trust boundaries even when they come from a normally trusted service.

Consumer repositories decide which facts to publish and which credentials to provide. Local users
control the configuration and destination; this does not authorize the generator to write outside
that destination. A public profile site must not expose private captures or credentials.

## Properties that must hold

- Untrusted text must not become executable SVG, HTML or script, nor leak through diagnostics.
- Credentials and unauthorized private metadata must not enter generated output or captured state.
- Imports must preserve their verified identity. Failed required imports must not silently substitute
  unrelated cached configuration or claim provenance they do not have.
- Package discovery and organization scoping must stay within the approved owners and content.
- Generation, recovery and archive extraction must respect filesystem and ownership boundaries,
  including path traversal, symlinks, competing writers and interrupted promotion.
- Parsing, collection and archive extraction must respect their resource limits.
- Installation must verify release identity, digests and required attestations before execution.

A violation of these properties can be reportable even when tests pass. Reports should explain the
input an attacker controls, how it reaches the affected behavior, and the resulting impact.
No finding class is excluded merely because the tool is run locally or the input comes from GitHub.

## Verification limits

Tests and build provenance are not guarantees that a release is free of vulnerabilities. The CLI is
not a sandbox for arbitrary local programs. Assess findings against the actual supported execution
path and permissions. Operational recovery and publication boundaries are documented in
[operations](docs/operations.md) and [distribution](docs/distribution.md).
