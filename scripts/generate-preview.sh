#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# No README or output location is inferred from the caller's working directory.
exec python3 "$repo_root/scripts/bootstrap_preview.py" "$@"
