#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
# Guard: if AEMEATH_PROJECT_DIR does not point at a project root, fall back to
# BASH_SOURCE. This keeps direct script execution and stale hook env values safe.
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi
cd "$ROOT"

echo "[hook-env] AEMEATH_PROJECT_DIR=${AEMEATH_PROJECT_DIR:-<unset>}"
echo "[hook-env] CLAUDE_PROJECT_DIR=${CLAUDE_PROJECT_DIR:-<unset>}"
echo "[hook-env] ROOT=$ROOT"
echo "[hook-env] PWD=$PWD"

# Keep hook builds isolated per checkout. Reusing the default/shared target dir
# across worktrees can leave stale crate metadata and make downstream crates see
# old public APIs from local path dependencies.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/hook-tests}"

packages=(
  share
  workflow
  runtime
  project
  policy
  context
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
