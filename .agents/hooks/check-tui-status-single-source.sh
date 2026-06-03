#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

fail=0

report_matches() {
  local message="$1"
  shift
  local tmp
  tmp="$(mktemp)"
  if "$@" >"$tmp"; then
    if [ -s "$tmp" ]; then
      cat "$tmp" >&2
      echo "[architecture] $message" >&2
      fail=1
    fi
  fi
  rm -f "$tmp"
}

# StatusBar 运行态镜像（model/session/tps/token/api/context_size/工作目录上下文）的真相归
# RuntimeModel/SessionModel。唯一生产写入路径为
# StatusViewAssembler::assemble_runtime_view -> StatusBar::apply_runtime_view，
# 由 adapter/status_widget.rs 调用。结构上禁止恢复分散 setter 或 ChatState token/api 镜像。
report_matches \
  "status_bar runtime mirror setters must not be reintroduced; derive StatusRuntimeViewModel from RuntimeModel/SessionModel and write via apply_runtime_view." \
  grep -RInE 'fn[[:space:]]+(set_model|set_session_id|set_tps|set_tokens|set_api_calls|set_context_size|set_context_paths|set_git_context)[[:space:]]*\(' \
    "$ROOT/apps/cli/src/tui/render/status" --include='*.rs'

report_matches \
  "ChatState must not mirror token/api usage; keep usage in RuntimeModel and derive status via StatusViewAssembler." \
  grep -RInE '(total_input_tokens|total_output_tokens|total_api_calls|last_input_tokens|usage_snapshot|record_usage)' \
    "$ROOT/apps/cli/src/tui/app/state" --include='*.rs'

exit "$fail"
