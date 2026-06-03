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

# Input selection truth lives in view_state.input_sel and InputArea must render from that
# projection directly; it must not physically store input selection mirrors. Status selection still
# keeps a render mirror pending a later stateless slice.

report_matches \
  "InputArea must not physically store input selection mirror fields; render directly from InputSelectionViewState." \
  grep -RInE 'pub\(super\)[[:space:]]+(is_selecting|selection_start|selection_end):' \
    "$ROOT/apps/cli/src/tui/render/input/input_area.rs" \
    "$ROOT/apps/cli/src/tui/render/input/input_area" --include='*.rs'

report_matches \
  "input selection mirrors must not be written through input_area/self; write view_state.input_sel and render from it." \
  bash -c "grep -RInE '\b(input_area|self)\.(is_selecting|selection_start|selection_end)\s*=' \"$ROOT/apps/cli/src/tui/render/input\" \"$ROOT/apps/cli/src/tui/adapter/input_widget.rs\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/view_state/' | grep -v 'view_state\.input_sel\.' | grep -v '\s*=='"

report_matches \
  "status_bar selection mirrors must not be written outside its adapter or widget internals; write view_state.status_sel instead." \
  bash -c "grep -RInE '\b(status_bar|self)\.(is_selecting|selection_start|selection_end|selection_row|selection_width)\s*=' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/view_state/' | grep -v '/adapter/status_widget.rs' | grep -v '/render/display/status_bar_selection.rs' | grep -v '/render/output_area/' | grep -v 'view_state\.status_sel\.' | grep -v '\s*=='"

report_matches \
  "InputArea/StatusBar must not expose production selection state mutators or selected-text getters that depend on widget mirrors; use selected_text_for_view." \
  bash -c "perl -ne 'BEGIN { \$pending=0 } if (/^\\s*#\\[cfg\\(test\\)\\]/) { \$pending=1; next } if (/pub[^\\(]*(fn\\s+(clear_selection|get_selected_text|start_selection|start_selection_at|update_selection|update_selection_at|end_selection)\\()/ && !\$pending) { print \"\$ARGV:\$.:\$_\" } \$pending=0' \"$ROOT/apps/cli/src/tui/render/input/input_area/selection.rs\" \"$ROOT/apps/cli/src/tui/render/display/status_bar_selection.rs\""

report_matches \
  "production copy path must not read input_area/status_bar.get_selected_text(); use selected_text_for_view(&view_state.*)." \
  grep -RInE '\b(input_area|status_bar)\.get_selected_text\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' --exclude='*_tests.rs' --exclude='input_widget.rs' --exclude='status_widget.rs'

report_matches \
  "input/status widget selection state methods must stay deleted; mouse handling should mutate view_state.input_sel/status_sel." \
  grep -RInE '\b(input_area|status_bar)\.(start_selection|start_selection_at|update_selection|update_selection_at|end_selection)\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' --exclude='*_tests.rs'

exit "$fail"
