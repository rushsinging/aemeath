#!/bin/bash
# Regression tests for the layered Agent Stop and Git pre-push gates.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
STOP_HOOK="$SCRIPT_DIR/check-agent-stop.sh"

# 隔离 git 注入的仓库本地环境变量：worktree 场景下 git 会注入
# GIT_DIR=<主仓>/.git/worktrees/<worktree>；一旦泄漏给本 hook 的子进程
# （guard 脚本 → cargo test → 测试 fixture 的 git init），git 会把主仓 config
# 判成 bare 仓库并写入 core.bare=true，导致主仓不可用。
while IFS= read -r git_local_var; do unset "$git_local_var"; done < <(git rev-parse --local-env-vars 2>/dev/null || true)
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
  mkdir -p "$repo/.agents/hooks" "$repo/.cargo/hooks" "$repo/scripts" "$repo/bin"
  git -C "$repo" init -q
  cp "$STOP_HOOK" "$repo/.agents/hooks/check-agent-stop.sh"
  cp "$PRE_PUSH_HOOK" "$repo/.cargo/hooks/pre-push"
  cp "$REPO_ROOT/scripts/clean-worktree-targets.sh" "$repo/scripts/clean-worktree-targets.sh"
  cp "$REPO_ROOT/.cargo/lib.sh" "$repo/.cargo/lib.sh"
  cp "$REPO_ROOT/.agents/hooks/run-xtask.sh" "$repo/.agents/hooks/run-xtask.sh"

  cat >"$repo/.agents/hooks/check-architecture-guards.sh" <<'EOF'
#!/usr/bin/env bash
printf 'architecture:%s\n' "${1:-missing}" >>"$GATE_LOG"
[ "${FAKE_FAIL:-}" != "architecture" ]
EOF
  # pre-push 经 `run-xtask.sh`；fixture 无 workspace 时回退 `cargo run -p xtask`。
  # fake repo 无 Cargo.toml，用 PATH stub 拦截 cargo 调用并记录 gate 序列。
  cat >"$repo/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == *"test-runner"* ]]; then
  printf 'unit-tests\n' >>"$GATE_LOG"
  [ "${FAKE_FAIL:-}" != "unit-tests" ]
  exit $?
fi
exit 0
EOF
  chmod +x "$repo/.agents/hooks/"*.sh "$repo/.cargo/hooks/pre-push" "$repo/scripts/clean-worktree-targets.sh" "$repo/bin/cargo"
}

run_gate() {
  local repo="$1"
  local hook="$2"
  local failure="${3:-}"
  (
    cd "$repo"
    PATH="$repo/bin:$PATH" GATE_LOG="$repo/gate.log" FAKE_FAIL="$failure" "$hook"
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
    HOME="$repo/home" /bin/bash scripts/clean-worktree-targets.sh --current --yes --dry-run \
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

  # #1966：纯 delete（local oid 全 0）跳过门禁，不清缓存。
  : >"$repo/gate.log"
  mkdir -p "$current_cache/build"
  touch "$current_cache/build/marker"
  zero_oid="$(printf '%040d' 0)"
  real_oid="$(printf '%040d' 1)"
  HOME="$repo/home" run_gate "$repo" "$repo/.cargo/hooks/pre-push" \
    <<<"refs/heads/tmp $zero_oid refs/heads/tmp $real_oid"
  [ -z "$(cat "$repo/gate.log")" ] \
    || fail "delete-only push must skip gates: $(cat "$repo/gate.log")"
  test -d "$current_cache" \
    || fail "delete-only push must not clean the current cache"

  # 混合 delete + 更新：仍跑完整门禁。
  : >"$repo/gate.log"
  HOME="$repo/home" run_gate "$repo" "$repo/.cargo/hooks/pre-push" <<EOF
refs/heads/gone $zero_oid refs/heads/gone $real_oid
refs/heads/feat $real_oid refs/heads/feat $zero_oid
EOF
  [ "$(cat "$repo/gate.log")" = "$(printf 'architecture:--full\nunit-tests')" ] \
    || fail "mixed delete+update must still run gates: $(cat "$repo/gate.log")"

  echo "layered hook regression tests passed"
}

main "$@"
