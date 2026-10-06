# Repository settings

These controls apply only to `kdominic89/sourcefield`. They are GitHub settings rather than a
workflow that silently rewrites repository policy. Inspect the live settings when changing them.

## Main and version tags

Main requires a pull request, resolved review conversations, linear history and these successful
GitHub Actions checks against the current base:

- `validate`
- `native-platforms (macos-15)`
- `native-platforms (windows-2025)`

Zero required reviewer approvals support the current sole-maintainer workflow; the owner has no
routine ruleset bypass. Force pushes and main deletion are blocked. Squash and rebase preserve a
linear history; merge commits are disabled. Auto-merge remains available for deliberately selected
PRs and does not constitute a blanket Dependabot auto-merge policy.

Two independent `v*` tag rulesets restrict creation to the owner and forbid updates/deletion without
bypass. The creation permission does not permit modifying an existing tag. For release approval,
see [distribution](distribution.md).

## Actions and scanning

Actions permits GitHub-owned and repository-owner/local Actions, requires full action SHAs, and
does not grant a blanket exception to verified Marketplace creators. Reusable workflows can still
use tag refs under GitHub's policy; Sourcefield separately validates consumer workflow/lock identity.
All external fork contributors require workflow execution approval. Default tokens remain read-only;
workflows cannot approve pull requests. Release jobs declare their narrower required write permissions.

CodeQL uses GitHub Default Setup with native merge protection for error alerts and security findings
rated high or critical within the pull-request diff. No CodeQL workflow file is needed. GitHub
exempts merge queue groups and Dependabot PRs analyzed by Default Setup from this native rule.
Existing alerts outside the diff do not block unrelated changes. Default Setup excludes fork PRs,
and a merge rule does not add the missing scans. Inspect the actual fork-PR outcome before claiming
CodeQL coverage or a usable merge path for those contributions. Never add an unreviewed bypass to
make a blocked contribution merge.

Secret scanning, push protection, Dependabot alerts/security updates and private vulnerability
reporting remain enabled. Report vulnerabilities using [SECURITY.md](../SECURITY.md).
Repository Custom / Security alerts notification selection is personal to the watching maintainer;
email delivery additionally depends on that account's notification preferences.

## Tool updates and verification

Dependabot covers Cargo, GitHub Actions, `rust-toolchain.toml` and the authored Playwright npm
manifest/lock. `npm ci` consumes the lock without changing it. CI retains the full Chromium build
and an AppArmor policy for its exact executable path rather than disabling the browser sandbox.

wasm-pack's exact pin lives in `tools/wasm-pack-version.txt`, consumed by local and CI builds.
A weekly read-only registry check reports a stale pin. Prepare a version change with
`python3 scripts/tool_versions.py --update`; installation, full verification and a reviewed PR are
separate steps. The check fails visibly on unavailable or invalid registry responses, so a failed
lookup is never recorded as evidence of freshness.

A settings readback proves configuration, not a new CI run, a completed Dependabot update, fork
behavior or release immutability. Verify those through their actual separately authorized runs.

## Primary references

- [Rulesets and parameters](https://docs.github.com/en/rest/repos/rules#create-a-repository-ruleset)
- [Code scanning merge protection](https://docs.github.com/en/code-security/how-tos/find-and-fix-code-vulnerabilities/manage-your-configuration/set-merge-protection)
- [Native merge-protection exceptions](https://docs.github.com/en/code-security/concepts/code-scanning/merge-protection#exceptions-and-limitations)
- [Default Setup coverage](https://docs.github.com/en/code-security/concepts/code-scanning/setup-types)
- [Actions permissions](https://docs.github.com/en/rest/actions/permissions)
- [Protected environments](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/manage-environments)
- [Dependabot ecosystems](https://docs.github.com/en/code-security/reference/supply-chain-security/supported-ecosystems-and-repositories)
- [npm ci](https://docs.npmjs.com/cli/v11/commands/npm-ci/)
