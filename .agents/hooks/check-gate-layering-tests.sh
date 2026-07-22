#!/usr/bin/env bash
# Regression tests for the layered Agent Stop and Git pre-push gates.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
STOP_HOOK="$SCRIPT_DIR/check-agent-stop.sh"
PRE_PUSH_HOOK="$REPO_ROOT/.cargo/hooks/pre-push"
tmp=""

cleanup() {
  if [ -n "$tmp" ]; then
    rm -rf -- "$tmp"
  fi
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

make_fake_repo() {
  local repo="$1"
  mkdir -p "$repo/.agents/hooks" "$repo/.cargo/hooks" "$repo/scripts"
  git -C "$repo" init -q
  cp "$STOP_HOOK" "$repo/.agents/hooks/check-agent-stop.sh"
  cp "$PRE_PUSH_HOOK" "$repo/.cargo/hooks/pre-push"
  cp "$REPO_ROOT/scripts/clean-worktree-targets.sh" "$repo/scripts/clean-worktree-targets.sh"
  cp "$REPO_ROOT/.cargo/lib.sh" "$repo/.cargo/lib.sh"

  cat >"$repo/.agents/hooks/check-architecture-guards.sh" <<'EOF'
#!/usr/bin/env bash
printf 'architecture:%s\n' "${1:-missing}" >>"$GATE_LOG"
[ "${FAKE_FAIL:-}" != "architecture" ]
EOF
  cat >"$repo/.agents/hooks/check-unit-tests.sh" <<'EOF'
#!/usr/bin/env bash
printf 'unit-tests\n' >>"$GATE_LOG"
[ "${FAKE_FAIL:-}" != "unit-tests" ]
EOF
  chmod +x "$repo/.agents/hooks/"*.sh "$repo/.cargo/hooks/pre-push" "$repo/scripts/clean-worktree-targets.sh"
}

run_gate() {
  local repo="$1"
  local hook="$2"
  local failure="${3:-}"
  (
    cd "$repo"
    GATE_LOG="$repo/gate.log" FAKE_FAIL="$failure" "$hook"
  )
}

main() {
  local repo output status
  tmp="$(mktemp -d)"
  repo="$tmp/repo"

  [ -x "$STOP_HOOK" ] || fail "missing executable Agent Stop hook: $STOP_HOOK"
  [ -x "$PRE_PUSH_HOOK" ] || fail "missing executable pre-push hook: $PRE_PUSH_HOOK"
  make_fake_repo "$repo"

  run_gate "$repo" "$repo/.agents/hooks/check-agent-stop.sh"
  [ "$(cat "$repo/gate.log")" = "architecture:--fast" ] \
    || fail "Agent Stop must run only fast architecture guards: $(cat "$repo/gate.log")"

  : >"$repo/gate.log"
  current_cache="$(
    cd "$repo"
    HOME="$repo/home" bash scripts/clean-worktree-targets.sh --current --yes --dry-run \
      | awk '/no current cache:/ {print $NF}'
  )"
  mkdir -p "$current_cache/build"
  touch "$current_cache/build/marker"
  HOME="$repo/home" run_gate "$repo" "$repo/.cargo/hooks/pre-push" </dev/null
  [ "$(cat "$repo/gate.log")" = "$(printf 'architecture:--full\nunit-tests')" ] \
    || fail "pre-push must run full guards before unit tests: $(cat "$repo/gate.log")"
  test ! -d "$current_cache" \
    || fail "pre-push must clean only the current worktree cache after all gates"

  : >"$repo/gate.log"
  mkdir -p "$current_cache/build"
  touch "$current_cache/build/marker"
  set +e
  output="$(HOME="$repo/home" run_gate "$repo" "$repo/.cargo/hooks/pre-push" architecture </dev/null 2>&1)"
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail "architecture failure must block pre-push"
  [ "$(cat "$repo/gate.log")" = "architecture:--full" ] \
    || fail "pre-push must fail fast before unit tests: $(cat "$repo/gate.log"); output=$output"
  test -d "$current_cache" \
    || fail "pre-push must not clean the current cache when a gate fails"

  echo "layered hook regression tests passed"
}

main "$@"
