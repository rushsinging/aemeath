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

# Output scroll/selection truth lives in view_state.output. OutputArea keeps only render-time mirrors:
# - scroll_offset / auto_scroll are written by adapter/output_view_widget.rs before render;
# - selection_start / selection_end / is_selecting are render highlight mirrors only;
# - copying selected text must read OutputViewState + document, not widget selection mirrors.

report_matches \
  "output(_area) scroll/selection mirrors must not be written outside output_view_widget.rs or OutputArea internals/tests; write view_state.output instead." \
  bash -c "grep -RInE '\b(output|output_area|self)\.(scroll_offset|auto_scroll|is_selecting|selection_start|selection_end)\s*=' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/view_state/' | grep -v '/render/output_area' | grep -v '/adapter/output_view_widget.rs' | grep -v '/render/input/input_area/selection.rs' | grep -v '/render/display/status_bar_selection.rs' | grep -v 'view_state\.output\.' | grep -v '/app/resize.rs:.*app\.output_area\.'"

report_matches \
  "OutputArea must not expose production selection state mutators or selected-text getters that depend on widget mirrors; use OutputViewState + selected_text_for_view." \
  bash -c "perl -ne 'BEGIN { \$pending=0 } if (/^\\s*#\\[cfg\\(test\\)\\]/) { \$pending=1; next } if (/pub[^\\(]*(fn\\s+(clear_selection|get_selected_text|start_selection|update_selection|end_selection|select_word)\\()/ && !\$pending) { print \"\$ARGV:\$.:\$_\" } \$pending=0' \"$ROOT/apps/cli/src/tui/render/output_area/selection.rs\""

report_matches \
  "production copy path must not read output_area.get_selected_text(); use output_area.selected_text_for_view(&view_state.output)." \
  grep -RInE 'output_area\.get_selected_text\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' --exclude='*_tests.rs'

report_matches \
  "output_area scroll methods must stay deleted; scrolling goes through view_state.output." \
  grep -RInE 'output_area\.(scroll_up|scroll_down|scroll_to_bottom|scroll_to_top)\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' --exclude='*_tests.rs'

exit "$fail"
