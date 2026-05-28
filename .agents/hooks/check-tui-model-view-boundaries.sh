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

# Model purity: model cannot depend on rendering/backend/effect execution protocols.
report_matches \
  "TUI model must stay pure: no ratatui, terminal backend, AgentClient, channels, spawn or filesystem command execution." \
  grep -RInE 'ratatui|Crossterm|Terminal<|AgentClient|mpsc::Sender|tokio::spawn\s*\(|std::thread::spawn\s*\(|Command::new\s*\(|clipboard::|arboard::|copypasta::|read_clipboard_image\s*\(|process_image_file\s*\(|Handle::block_on\s*\(|Runtime::block_on\s*\(|block_in_place|\.await\b' \
    "$ROOT/apps/cli/src/tui/model" --include='*.rs'

# Render isolation: render consumes view_model/view_state/output render data, not model mutation intents/changes.
report_matches \
  "TUI render must not contain legacy fallback that marks the last running tool as complete." \
  grep -RInE 'find_last_running_tool|last running|最后一个 running' \
    "$ROOT/apps/cli/src/tui/render" --include='*.rs'

# ViewAssembler boundary.
report_matches \
  "TUI view_assembler must not render with ratatui or execute side effects." \
  grep -RInE 'ratatui|tokio::spawn\s*\(|std::thread::spawn\s*\(|Command::new\s*\(|mpsc::Sender|\.await\b|HookRunner::run|\.run_hook\s*\(' \
    "$ROOT/apps/cli/src/tui/view_assembler" --include='*.rs'

# ViewModel dependency guard.
report_matches \
  "TUI view_model must not depend on model internals or ratatui." \
  grep -RInE 'crate::tui::model|ratatui' \
    "$ROOT/apps/cli/src/tui/view_model" --include='*.rs'

# Adapter boundary: SDK/runtime event protocols are accepted only in adapter or app edge modules.
report_matches \
  "SDK/runtime event protocols must be adapted before reaching TUI model/conversation." \
  grep -RInE 'sdk::ChatEvent|RuntimeStreamEvent' \
    "$ROOT/apps/cli/src/tui/model" "$ROOT/apps/cli/src/tui/view_model" "$ROOT/apps/cli/src/tui/view_assembler" "$ROOT/apps/cli/src/tui/render" --include='*.rs'

# Physical legacy guards for feature #55.
if [ -d "$ROOT/apps/cli/src/tui/core/state" ] || [ -d "$ROOT/apps/cli/src/tui/core/update" ]; then
  echo "[architecture] legacy tui/core/state or tui/core/update directory is forbidden after feature #55" >&2
  fail=1
fi

if [ -d "$ROOT/apps/cli/src/tui/model/session" ]; then
  echo "[architecture] tui/model/session is not a fifth model context; session model belongs under runtime" >&2
  fail=1
fi

if [ -f "$ROOT/apps/cli/src/tui/output_area/markdown.rs" ] \
  || [ -f "$ROOT/apps/cli/src/tui/output_area/rendered_lines.rs" ] \
  || [ -f "$ROOT/apps/cli/src/tui/output_area/render_blocks.rs" ] \
  || [ -f "$ROOT/apps/cli/src/tui/output_area/render_spans.rs" ] \
  || [ -f "$ROOT/apps/cli/src/tui/output_area/render_status.rs" ] \
  || [ -f "$ROOT/apps/cli/src/tui/output_area/diff.rs" ] \
  || [ -d "$ROOT/apps/cli/src/tui/output_area/tool_display" ]; then
  echo "[architecture] output render implementation must live under tui/render/output after feature #55" >&2
  fail=1
fi

exit "$fail"
