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
# - visible height / total document lines / scroll_offset / auto_scroll are maintained by OutputViewState;
# - selection_start / selection_end / is_selecting are render highlight mirrors only;
# - copying selected text must read OutputViewState + document, not widget selection mirrors.

report_matches \
  "output_widget/output_view_widget adapters must stay retired and test-only; keep production projection in App/ViewState, not widget adapters." \
  bash -c "perl -ne 'BEGIN { \$pending=0 } if (/^\\s*#\\[cfg\\(test\\)\\]/) { \$pending=1; next } if (/^\\s*(\\/\\/.*)?\$/) { next } if (/pub[[:space:]]+mod[[:space:]]+(output_widget|output_view_widget)[[:space:]]*;/ && !\$pending) { print \"\$ARGV:\$.:\$_\" } \$pending=0' \"$ROOT/apps/cli/src/tui/adapter.rs\""

report_matches \
  "output_view_widget adapter must stay retired; synchronize scroll via OutputViewState::sync_document_metrics from App/layout, not widget readback." \
  bash -c "perl -ne 'next if /^\\s*\\/\\// || /^\\s*#!?\\[/ || /^\\s*#\\[cfg\\(test\\)\\]/ || /^\\s*mod tests/ || /^\\s*use / || /^\\s*fn / || /^\\s*let / || /^\\s*assert/ || /sync_document_metrics/; if (/(pub\\(crate\\)[[:space:]]+fn|OutputArea|&mut[[:space:]]+OutputViewState|output_area\\.|\\.last_visible_height[[:space:]]*=)/) { print \"\$ARGV:\$.:\$_\" }' \"$ROOT/apps/cli/src/tui/adapter/output_view_widget.rs\""

report_matches \
  "OutputArea must not keep scroll metrics mirrors; visible height and last document total lines live in OutputViewState." \
  bash -c "grep -RInE 'last_visible_height|last_line_count' \"$ROOT/apps/cli/src/tui/render/output_area.rs\" \"$ROOT/apps/cli/src/tui/render/output_area\" --include='*.rs' | grep -v '/render/output_area/render.rs:.*view\.last_visible_height' | grep -v '/render/output_area/render.rs:.*last_visible_height:' || true"

report_matches \
  "OutputArea::handle_resize must not receive or store visible height hints; App updates OutputViewState instead." \
  bash -c "grep -RInE 'handle_resize\\([^)]*visible_height|visible_height_hint|output_area\\.last_visible_height' \"$ROOT/apps/cli/src/tui\" --include='*.rs' || true"

report_matches \
  "output(_area) scroll/selection mirrors must not be written outside OutputArea internals/tests; write view_state.output instead." \
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

# Output document projection is owned by App's render pipeline:
# - OutputDocumentRenderer::render_model_document builds a RenderedDocument from OutputViewModel + renderer cache;
# - adapter/output_widget.rs remains retired and must not contain production helpers;
# - production paths replace OutputArea.document only through refresh_output_document_from_model.

report_matches \
  "output_widget adapter must stay retired; keep production projection in OutputArea/App and do not reintroduce widget writeback." \
  bash -c "perl -ne 'next if /^\\s*\\/\\// || /^\\s*#!?\\[/ || /^\\s*#\\[cfg\\(test\\)\\]/ || /^\\s*mod tests/ || /^\\s*use / || /^\\s*fn / || /^\\s*let / || /^\\s*assert/ || /renderer\\.render_model_document/ || /OutputArea::new/ || /output_area\\.replace_document\\(document\\)/; if (/(pub\\(crate\\)[[:space:]]+fn|render_document_from_view_model|render_output_document|&mut[[:space:]]+OutputArea|\\.replace_document\\(|\\.set_document\\()/) { print \"\$ARGV:\$.:\$_\" }' \"$ROOT/apps/cli/src/tui/adapter/output_widget.rs\""

report_matches \
  "OutputArea must not own the renderer cache; App owns OutputDocumentRenderer and applies RenderedDocument centrally." \
  grep -RInE 'pub[[:space:]]+document_renderer|[[:space:]]document_renderer[[:space:]]*:' \
    "$ROOT/apps/cli/src/tui/render/output_area.rs"

report_matches \
  "production output refresh must use refresh_output_document_from_model, not the retired refresh_output_widget_from_model name." \
  grep -RInE 'refresh_output_widget_from_model' \
    "$ROOT/apps/cli/src/tui" --include='*.rs'

report_matches \
  "production OutputArea document replacement must stay centralized in app/update.rs; tests may call replace_document directly." \
  bash -c "grep -RInE 'output_area\\.replace_document\\(|\\barea\\.replace_document\\(|\\boutput\\.replace_document\\(' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/app/update.rs:' | grep -v '/render/output_area.rs:' | grep -v '/render/output_area/render.rs:' | grep -v '/render/output_area/selection.rs:' | grep -v '/render/output/selection_tests.rs:' | grep -v '/adapter/output_widget.rs:'"

report_matches \
  "the retired OutputArea::set_document API must not be restored; use replace_document in the centralized render pipeline/tests." \
  bash -c "grep -RInE '\\.[[:space:]]*set_document[[:space:]]*\\(' \"$ROOT/apps/cli/src/tui\" --include='*.rs' || true"

exit "$fail"
