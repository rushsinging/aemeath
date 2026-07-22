#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT/scripts/clean-worktree-targets.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

repo="$tmp/repo"
mkdir -p "$repo/scripts" "$repo/.cargo"
cp "$SCRIPT" "$repo/scripts/clean-worktree-targets.sh"
cp "$ROOT/.cargo/lib.sh" "$repo/.cargo/lib.sh"
chmod +x "$repo/scripts/clean-worktree-targets.sh"
git -C "$repo" init -q
git -C "$repo" config user.email test@example.com
git -C "$repo" config user.name test
touch "$repo/README"
git -C "$repo" add README
git -C "$repo" commit -qm initial
git -C "$repo" branch -M main

home="$tmp/home"
current_cache="$(
  cd "$repo"
  HOME="$home" bash scripts/clean-worktree-targets.sh --current --yes --dry-run \
    | awk '/no current cache:/ {print $NF}'
)"
other_key="other-0123456789abcdef"
mkdir -p "$current_cache/build"
mkdir -p "$home/.cache/aemeath-target/$other_key/build"
touch "$current_cache/build/current-marker"
touch "$home/.cache/aemeath-target/$other_key/build/other-marker"

(
  cd "$repo"
  HOME="$home" bash scripts/clean-worktree-targets.sh --current --yes
)

test ! -e "$current_cache"
test -e "$home/.cache/aemeath-target/$other_key/build/other-marker"

echo "current target cleanup test passed"
