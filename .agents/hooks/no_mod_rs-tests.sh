#!/usr/bin/env bash
# Regression tests for no_mod_rs.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOOK="$SCRIPT_DIR/no_mod_rs.sh"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

run_hook() {
  local root="$1"
  AEMEATH_PROJECT_DIR="$root" "$HOOK" 2>&1
}

main() {
  local tmp repo output status
  tmp="$(mktemp -d)"
  trap "rm -rf '$tmp'" EXIT

  repo="$tmp/repo"
  mkdir -p "$repo/.agents/hooks" \
    "$repo/.worktrees/linked/agent/features/runtime/src/ports" \
    "$repo/.claude/cache/src/ignored" \
    "$repo/target/debug/build/src/ignored"
  touch "$repo/.worktrees/linked/agent/features/runtime/src/ports/mod.rs"
  touch "$repo/.claude/cache/src/ignored/mod.rs"
  touch "$repo/target/debug/build/src/ignored/mod.rs"

  output="$(run_hook "$repo")" || fail "ignored directories must not fail the guard: $output"
  printf '%s' "$output" | grep -q 'OK: 未发现 mod.rs 文件' \
    || fail "successful output should confirm no mod.rs: $output"

  mkdir -p "$repo/agent/features/runtime/src/ports"
  touch "$repo/agent/features/runtime/src/ports/mod.rs"
  set +e
  output="$(run_hook "$repo")"
  status=$?
  set -e
  [ "$status" -eq 1 ] || fail "real source mod.rs must fail with exit 1, got $status; output=$output"
  printf '%s' "$output" | grep -q "$repo/agent/features/runtime/src/ports/mod.rs" \
    || fail "failure must report the real source path: $output"
  if printf '%s' "$output" | grep -q '/.worktrees/'; then
    fail "failure output must not include pruned worktrees: $output"
  fi

  echo "no_mod_rs hook regression tests passed"
}

main "$@"
