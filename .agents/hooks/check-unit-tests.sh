#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

commands=(
  "cargo test -p kernel --lib"
  "cargo test -p provider --lib"
  "cargo test -p tool --lib"
  "cargo test -p cli --bin aemeath"
)

for command in "${commands[@]}"; do
  echo "==> ${command}"
  eval "$command"
  echo
done
