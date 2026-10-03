#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# The historical entry point now enforces the same mandatory checks as CI.
exec "$repo_root/scripts/verify.sh" "$@"
