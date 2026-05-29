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

# StatusBar 运行态镜像（model/session/tps/工作目录上下文）的真相归 RuntimeModel/SessionModel。
# 唯一生产写入路径为 StatusViewAssembler::assemble_runtime_view -> StatusBar::apply_runtime_view，
# 由 adapter/status_widget.rs 调用。任何 update/effect/slash 业务路径都不得直接调用
# status_bar.set_model / set_session_id / set_tps / set_tokens / set_context_paths / set_git_context。
report_matches \
  "status_bar runtime mirror mutations (set_model/set_session_id/set_tps/set_tokens/set_context_paths/set_git_context) are allowed only in adapter/status_widget.rs; send a RuntimeIntent/SessionIntent and let the assembler + apply_runtime_view write the widget." \
  grep -RInE 'status_bar\.(set_model|set_session_id|set_tps|set_tokens|set_context_paths|set_git_context)\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude='status_widget.rs'

exit "$fail"
