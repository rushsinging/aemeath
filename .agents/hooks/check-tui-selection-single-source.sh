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

# Input/status/output selection truth lives in view_state and widgets must render from those
# projections directly; they must not physically store selection mirrors.

report_matches \
  "InputArea must not physically store input selection mirror fields; render directly from InputSelectionViewState." \
  grep -RInE 'pub\(super\)[[:space:]]+(is_selecting|selection_start|selection_end):' \
    "$ROOT/apps/cli/src/tui/render/input/input_area.rs" \
    "$ROOT/apps/cli/src/tui/render/input/input_area" --include='*.rs'

report_matches \
  "input selection mirrors must not be written through input_area/self; write view_state.input_sel and render from it." \
  bash -c "grep -RInE '\b(input_area|self)\.(is_selecting|selection_start|selection_end)\s*=' \"$ROOT/apps/cli/src/tui/render/input\" \"$ROOT/apps/cli/src/tui/adapter/input_widget.rs\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/view_state/' | grep -v 'view_state\.input_sel\.' | grep -v '\s*=='"

report_matches \
  "StatusBar must not physically store status selection mirror fields; render directly from StatusSelectionViewState." \
  grep -RInE '^[[:space:]]*(pub\((super|crate)\)[[:space:]]+)?(is_selecting|selection_start|selection_end|selection_row|selection_width):' \
    "$ROOT/apps/cli/src/tui/render/status" \
    "$ROOT/apps/cli/src/tui/render/display/status_bar_selection.rs" --include='*.rs'

report_matches \
  "status selection mirrors must not be written through status_bar/self; write view_state.status_sel and render from it." \
  bash -c "grep -RInE '\b(status_bar|self)\.(is_selecting|selection_start|selection_end|selection_row|selection_width)\s*=' \"$ROOT/apps/cli/src/tui/render/status\" \"$ROOT/apps/cli/src/tui/render/display/status_bar_selection.rs\" \"$ROOT/apps/cli/src/tui/adapter/status_widget.rs\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/view_state/' | grep -v 'view_state\.status_sel\.' | grep -v '\s*=='"

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

report_matches \
  "OutputArea must not physically store output selection/scroll mirror fields; render directly from OutputViewState." \
  grep -RInE '^[[:space:]]*pub[[:space:]]+(scroll_offset|auto_scroll|is_selecting|selection_start|selection_end):' \
    "$ROOT/apps/cli/src/tui/render/output_area.rs" \
    "$ROOT/apps/cli/src/tui/render/output_area" --include='*.rs'

report_matches \
  "output selection/scroll mirrors must not be written through output_area/self; write view_state.output and render from it." \
  bash -c "grep -RInE '\b(output_area|output|self)\.(scroll_offset|auto_scroll|is_selecting|selection_start|selection_end)\s*=' \"$ROOT/apps/cli/src/tui/render/output_area\" \"$ROOT/apps/cli/src/tui/adapter/output_view_widget.rs\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/view_state/' | grep -v 'view_state\.output\.' | grep -v '\s*=='"

report_matches \
  "OutputArea must not expose production selection state mutators or selected-text getters that depend on widget mirrors; use selected_text_for_view." \
  bash -c "perl -ne 'BEGIN { \$pending=0 } if (/^\\s*#\\[cfg\\(test\\)\\]/) { \$pending=1; next } if (/pub[^\\(]*(fn\\s+(clear_selection|get_selected_text|start_selection|start_selection_at|update_selection|update_selection_at|end_selection|set_selection_for_test)\\()/ && !\$pending) { print \"\$ARGV:\$.:\$_\" } \$pending=0' \"$ROOT/apps/cli/src/tui/render/output_area/selection.rs\" \"$ROOT/apps/cli/src/tui/render/output_area/render.rs\""

report_matches \
  "production copy path must not read output_area.get_selected_text(); use selected_text_for_view(&view_state.output)." \
  grep -RInE '\boutput_area\.get_selected_text\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' --exclude='*_tests.rs' --exclude='output_view_widget.rs'

exit "$fail"
