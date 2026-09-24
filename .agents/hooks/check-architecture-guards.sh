#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
# 守卫：如果 AEMEATH_PROJECT_DIR 不包含 .agents/hooks 说明不是项目根目录，
# 回退到 BASH_SOURCE 推导
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi
HOOKS_DIR="$ROOT/.agents/hooks"

# 守卫引擎切换（#1675 并存期）：AEMEATH_GUARD_ENGINE=xtask 时薄壳直接转发
# xtask guard，跳过旧脚本编排；默认留空走旧链路，直至分批迁移完成。
if [ "${AEMEATH_GUARD_ENGINE:-}" = "xtask" ]; then
  mode="${1:---full}"
  case "$mode" in
    --fast) exec cargo run --quiet -p xtask -- guard --fast ;;
    --full) exec cargo run --quiet -p xtask -- guard --full ;;
    *) exec cargo run --quiet -p xtask -- guard "$@" ;;
  esac
fi



mode="${1:---full}"
case "$mode" in
  --fast|--full) ;;
  *)
    echo "Usage: $0 [--fast|--full]" >&2
    exit 2
    ;;
esac

fast_pids=()
fast_names=()
fast_outputs=()
fast_status=0

# 防挂保护：任何 guard 都不得无限运行（Stop Hook 被无限 guard 阻断的根因修复）。
# macOS 无 GNU timeout，兼容 gtimeout；两者都没有时退回无超时（打印警告）。
if command -v timeout >/dev/null 2>&1; then
  TIMEOUT_BIN=timeout
elif command -v gtimeout >/dev/null 2>&1; then
  TIMEOUT_BIN=gtimeout
else
  TIMEOUT_BIN=""
  echo "[architecture] warning: neither timeout nor gtimeout found; guards run without timeout" >&2
fi

# guard 统一用 /bin/bash（macOS 系统 bash 3.2）执行：
# bash 5.x 的 here-doc 通过匿名管道传递，大量并发 fork 时写进程间歇性卡死
# （fork 后不 exec、管道写端泄漏——实测 51 并发 + 200 行 here-doc 必卡，
#   小 here-doc 与串行场景也偶发；bash 3.2 用临时文件实现 here-doc，无此问题，
#   实测 51 个 guard 全并发稳定通过）。
# 注意：guard 脚本 shebang（#!/usr/bin/env bash 会解析到 5.x），此处显式 /bin/bash 忽略 shebang。
# --kill-after=5：timeout 超时发 SIGTERM 后 5s 仍未退出则 SIGKILL（含进程组内子进程），
# 避免卡死 guard 的子进程残留并持有管道写端（aemeath hook 执行器侧兜底见 issue 1507）。
guarded() {
  if [ -z "$TIMEOUT_BIN" ]; then
    /bin/bash "$@"
  elif [ "$(type -t "$1")" = "function" ]; then
    local fn_name="$1"
    shift
    (
      export -f "$fn_name"
      export ROOT
      "$TIMEOUT_BIN" --kill-after=5 "${GUARD_TIMEOUT:-120}" /bin/bash -c "$fn_name"
    )
  else
    if [ "$1" = "bash" ]; then
      shift
    fi
    "$TIMEOUT_BIN" --kill-after=5 "${GUARD_TIMEOUT:-120}" /bin/bash "$@"
  fi
}

run_guard() {
  local profile="$1"
  shift
  local cmd="$1"

  if [ "$mode" = "--fast" ]; then
    if [ "$profile" = "fast" ]; then
      local tmp_out
      tmp_out="$(mktemp)"
      guarded "$@" >"$tmp_out" 2>&1 &
      fast_pids+=("$!")
      fast_names+=("$cmd")
      fast_outputs+=("$tmp_out")
    fi
    return
  fi

  guarded "$@"
}

wait_for_fast_guards() {
  local index status=0
  for index in "${!fast_pids[@]}"; do
    local wait_status=0
    wait "${fast_pids[$index]}" || wait_status=$?
    if [ "$wait_status" -ne 0 ]; then
      echo "[architecture] fast guard failed (exit=$wait_status): ${fast_names[$index]}" >&2
      if [ -s "${fast_outputs[$index]}" ]; then
        echo "--- ${fast_names[$index]} output ---" >&2
        cat "${fast_outputs[$index]}" >&2
        echo "--- end ${fast_names[$index]} ---" >&2
      fi
      status=1
    fi
    rm -f "${fast_outputs[$index]}"
  done
  return "$status"
}

echo "[hook-env] AEMEATH_PROJECT_DIR=${AEMEATH_PROJECT_DIR:-<unset>}"
echo "[hook-env] CLAUDE_PROJECT_DIR=${CLAUDE_PROJECT_DIR:-<unset>}"
echo "[hook-env] ROOT=$ROOT"
echo "[hook-env] ARCHITECTURE_GUARD_MODE=$mode"
run_guard full "$HOOKS_DIR/check-guard-registry.sh"
run_guard fast "$HOOKS_DIR/check-share-no-upstream-deps.sh"
run_guard fast "$HOOKS_DIR/check-noninteractive-child-session.sh"
run_guard full bash "$HOOKS_DIR/check-noninteractive-child-session-tests.sh"
run_guard fast "$HOOKS_DIR/check-task-state-pipeline.sh"
run_guard fast "$HOOKS_DIR/check-provider-http-attempt.sh"
run_guard fast "$HOOKS_DIR/check-provider-retry-ownership.sh"
run_guard fast "$HOOKS_DIR/check-provider-usage-capability.sh"
run_guard fast "$HOOKS_DIR/check-session-project-scope.sh"
run_guard fast "$HOOKS_DIR/check-hook-target-facade.sh"
run_guard fast "$HOOKS_DIR/check-tui-output-legacy-guards.sh"
run_guard fast "$HOOKS_DIR/check-tui-retained-output-view.sh"
run_guard fast "$HOOKS_DIR/check-tui-unsafe-text-ops.sh"
run_guard full "$HOOKS_DIR/check-log-target-prefix.sh"
run_guard full "$HOOKS_DIR/check-sdk-wire-schema.sh"
run_guard fast "$HOOKS_DIR/check-runtime-large-file-responsibilities.sh"
run_guard fast "$HOOKS_DIR/check-cost-tracker-retirement.sh"
run_guard fast "$HOOKS_DIR/check-runtime-event-naming.sh"
run_guard full bash "$HOOKS_DIR/check-runtime-event-naming-tests.sh"
run_guard full bash "$HOOKS_DIR/check-runtime-large-file-responsibilities-tests.sh"
run_guard full bash "$HOOKS_DIR/check-cost-tracker-retirement-tests.sh"
run_guard full "$HOOKS_DIR/check-production-reachability.sh"

if [ "$mode" = "--fast" ]; then
  wait_for_fast_guards || fast_status=1
fi

if [ "$fast_status" -eq 0 ]; then
  echo "All ${mode#--} architecture guards passed."
fi
exit "$fast_status"
