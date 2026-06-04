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

# StatusBar 必须保持 runtime/diagnostic/status stateless：
# - runtime/token/tps/model/session/context 真相归 RuntimeModel/SessionModel
# - status notice 真相归 RuntimeModel.status_notice
# - diagnostic severity/text 真相归 DiagnosticModel
# - thinking 开关真相归 RuntimeModel.thinking
# 渲染/选区路径只能消费 StatusViewModel，不得恢复 widget mirror 或写回 adapter。
report_matches \
  "StatusBar must not store runtime/diagnostic/status widget mirrors; derive StatusViewModel from model at render time." \
  grep -RInE '^[[:space:]]*(pub\([^)]*\)[[:space:]]+|pub[[:space:]]+)?(status|status_type|vm|thinking)[[:space:]]*:' \
    "$ROOT/apps/cli/src/tui/render/status" --include='*.rs'

report_matches \
  "StatusBar runtime/status setters must stay deleted; update RuntimeModel/DiagnosticModel and render StatusViewModel instead." \
  grep -RInE 'fn[[:space:]]+(set_success|set_warning|reset_runtime_state|set_thinking|apply_runtime_view|init|set_model|set_session_id|set_tps|set_tokens|set_api_calls|set_context_size|set_context_paths|set_git_context)[[:space:]]*\(' \
    "$ROOT/apps/cli/src/tui/render/status" --include='*.rs'

report_matches \
  "status_widget adapter must remain retired and test-only; do not reintroduce production export or widget writeback functions." \
  bash -c "perl -ne 'BEGIN { \$pending=0 } if (/^\\s*#\\[cfg\\(test\\)\\]/) { \$pending=1; next } if (/^\\s*(\\/\\/.*)?\$/) { next } if (/pub[[:space:]]+mod[[:space:]]+status_widget[[:space:]]*;/ && !\$pending) { print \"\$ARGV:\$.:\$_\" } \$pending=0' \"$ROOT/apps/cli/src/tui/adapter.rs\"; grep -nE 'fn[[:space:]]+apply_(runtime|diagnostic)_status_to_widget[[:space:]]*\\(' \"$ROOT/apps/cli/src/tui/adapter/status_widget.rs\" || true"

report_matches \
  "ChatState must not mirror token/api/thinking usage; keep usage/thinking in RuntimeModel and derive status via StatusViewAssembler." \
  grep -RInE '(total_input_tokens|total_output_tokens|total_api_calls|last_input_tokens|usage_snapshot|record_usage|thinking_enabled)' \
    "$ROOT/apps/cli/src/tui/app/state" --include='*.rs'

exit "$fail"
