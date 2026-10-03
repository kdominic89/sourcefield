# Primary implementation sources

Checked on 2026-10-03. These references establish external API behavior, not evidence that a hosted
release or deployment has already run.

- Reusable workflows and immutable commit references: https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows
- Caller/callee permissions: https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations
- Reusable job identity: https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#example-usage-of-job-context-workflow-identity
- Immutable releases: https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases
- Release integrity verification: https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/verify-release-integrity
- Build provenance: https://docs.github.com/en/actions/concepts/security/artifact-attestations
- Attestation verification constraints: https://cli.github.com/manual/gh_attestation_verify
- Immutable-release settings and required administration-read permission: https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository
- Native runner labels and architectures: https://docs.github.com/en/actions/reference/runners/github-hosted-runners
- Concurrency: https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency
- Commit-pinned repository contents: https://docs.github.com/en/rest/repos/contents#get-repository-content
- Reference resolution: https://docs.github.com/en/rest/commits/commits#get-a-commit
- Strict Serde models: https://serde.rs/container-attrs.html
- TOML: https://toml.io/en/
- Semantic versioning: https://semver.org/

Workflow action revisions are full commit pins. Update them through a reviewed dependency change and
verify vendor release provenance; major-version labels in comments are explanatory only. Rust and
WASM tool versions are pinned in repository configuration and `scripts/build-wasm.sh`.

- Playwright CI/browser provisioning: https://playwright.dev/docs/ci
- Exact existing browser test tool metadata: https://registry.npmjs.org/playwright/1.62.1
  (primary registry returned version 1.62.1 and exact playwright-core dependency 1.62.1; checked 2026-10-03).

- Dependabot ecosystems, weekly schedules and grouped Actions updates: https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference
