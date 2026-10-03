#!/usr/bin/env bash
set -euo pipefail
if [[ $# -ne 1 ]]; then
  printf '%s\n' 'Usage: scripts/serve.sh /absolute/path/to/generated/docs' >&2
  exit 2
fi

# Bind locally: previews can contain intentionally unpublished consumer content.
exec python3 -m http.server "${PORT:-8080}" --bind 127.0.0.1 --directory "$1"
