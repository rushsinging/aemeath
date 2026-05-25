#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

packages=(
  core
  runtime
  project
  policy
  prompt
  provider
  tools
  storage
  hook
  audit
  cli
)

for package in "${packages[@]}"; do
  if [[ "$package" == "cli" ]]; then
    echo "==> cargo test -p cli --bin aemeath"
    cargo test -p cli --bin aemeath
  else
    echo "==> cargo test -p ${package} --lib"
    cargo test -p "$package" --lib
  fi
  echo
done
