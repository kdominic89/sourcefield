#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

wasm_pack_version="$(python3 scripts/tool_versions.py --version)"

if ! command -v wasm-pack >/dev/null 2>&1; then
  printf 'wasm-pack %s is required; install with cargo install wasm-pack --version %s --locked.\n' \
    "$wasm_pack_version" "$wasm_pack_version" >&2
  exit 1
fi

if [[ "$(wasm-pack --version)" != "wasm-pack $wasm_pack_version" || ! -f Cargo.lock ]]; then
  printf 'wasm-pack %s and the committed Cargo.lock are required.\n' "$wasm_pack_version" >&2
  exit 1
fi

# Trailing arguments are forwarded to cargo; dependency resolution must stay locked.
wasm-pack build crates/sourcefield-wasm \
  --target web \
  --release \
  --out-dir ../../runtime/pkg \
  --out-name sourcefield_wasm \
  --locked

test -s runtime/pkg/sourcefield_wasm_bg.wasm
test -s runtime/pkg/sourcefield_wasm.js

# Bind the complete runtime to the same production sources consumed by the native build.
python3 scripts/runtime_manifest.py
