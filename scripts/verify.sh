#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

for tool in python3 node cargo rustc; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Required verification tool is missing: %s\n' "$tool" >&2
    exit 1
  fi

done

if [[ ! -f Cargo.lock ]]; then
  printf '%s\n' 'The committed Cargo.lock is required; verification never regenerates it.' >&2
  exit 1
fi

python3 -B -m unittest discover -s tests -p 'test_*.py'
python3 -B - <<'PY'
import ast
from pathlib import Path

for directory in (Path("scripts"), Path("tests")):
    for path in directory.rglob("*.py"):
        ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
PY
node --check runtime/app.js
node --check runtime/simulation-fallback.js
node --test tests/*.test.mjs

for module in scripts/*.mjs tests/integration/*.mjs; do
  node --check "$module"
done

for script in scripts/*.sh; do
  bash -n "$script"
done

cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo check -p sourcefield-wasm --target wasm32-unknown-unknown --locked
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D warnings" cargo doc --workspace --no-deps --document-private-items --locked
