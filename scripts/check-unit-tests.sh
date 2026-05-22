#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT"

commands=(
  "cargo test -p aemeath-core --lib"
  "cargo test -p aemeath-llm --lib"
  "cargo test -p aemeath-tools --lib"
  "cargo test -p aemeath-cli --bin aemeath"
  "cargo test -p server --lib"
  "cargo test -p agents --lib"
)

for command in "${commands[@]}"; do
  echo "==> ${command}"
  eval "$command"
  echo
done
