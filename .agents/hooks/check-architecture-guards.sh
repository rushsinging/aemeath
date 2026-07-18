#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
# 守卫：如果 AEMEATH_PROJECT_DIR 不包含 .agents/hooks 说明不是项目根目录，
# 回退到 BASH_SOURCE 推导
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi
HOOKS_DIR="$ROOT/.agents/hooks"

run_tui_single_source_structure_guard() {
  local fail=0

  report_matches() {
    local message="$1"
    shift
    local tmp
    tmp="$(mktemp)"
    "$@" >"$tmp" || true
    if [ -s "$tmp" ]; then
      cat "$tmp" >&2
      echo "[architecture] $message" >&2
      fail=1
    fi
    rm -f "$tmp"
  }

  # #70 structural single-source rule: app/domain truth lives in model/ or view_state/;
  # render widgets may only keep render projection/cache and retired adapters must stay test-only.
  report_matches \
    "retired TUI widget adapters must remain #[cfg(test)] only; production projection lives in model/view_state/render." \
    bash -c "perl -ne 'BEGIN { \$pending=0 } if (/^\\s*#\\[cfg\\(test\\)\\]/) { \$pending=1; next } if (/^\\s*(\\/\\/.*)?\$/) { next } if (/pub[[:space:]]+mod[[:space:]]+(input_widget|resize|live_status_widget|status_widget|output_widget|output_view_widget)[[:space:]]*;/ && !\$pending) { print \"\$ARGV:\$.:\$_\" } \$pending=0' \"$ROOT/apps/cli/src/tui/adapter.rs\""

  report_matches \
    "retired TUI widget adapters must not restore production writeback/helper APIs." \
    bash -c "perl -ne 'next if /^\\s*\\/\\// || /^\\s*#!?\\[/ || /^\\s*#\\[cfg\\(test\\)\\]/ || /^\\s*mod tests/ || /^\\s*use / || /^\\s*fn / || /^\\s*let / || /^\\s*assert/ || /sync_document_metrics/ || /renderer\\.render_model_document/ || /OutputArea::new/ || /output_area\\.replace_document\\(document\\)/; if (/(pub[[:space:]]*(\\([^)]*\\))?[[:space:]]*(struct|fn)|submission_from_changes|ResizeMapping|map_resize|apply_resize|apply_(live_status|runtime_status|diagnostic_status)_to_widget|render_document_from_view_model|render_output_document|&mut[[:space:]]+(InputArea|StatusBar|OutputArea|OutputViewState)|\\.(replace_document|set_document)\\()/) { print \"\$ARGV:\$.:\$_\" }' \"$ROOT/apps/cli/src/tui/adapter/input_widget.rs\" \"$ROOT/apps/cli/src/tui/adapter/resize.rs\" \"$ROOT/apps/cli/src/tui/adapter/live_status_widget.rs\" \"$ROOT/apps/cli/src/tui/adapter/status_widget.rs\" \"$ROOT/apps/cli/src/tui/adapter/output_widget.rs\" \"$ROOT/apps/cli/src/tui/adapter/output_view_widget.rs\""

  report_matches \
    "TUI render widgets must not physically store app/domain mirror fields; keep truth in model/ or view_state/." \
    bash -c "grep -RInE '^[[:space:]]*(pub(\\([^)]*\\))?[[:space:]]+)?(textarea|history|history_index|saved_input|status_type|vm|thinking|is_selecting|selection_start|selection_end|selection_row|selection_width|spinner|task_status_lines|queued_submission_lines)[[:space:]]*:' \"$ROOT/apps/cli/src/tui/render/input/input_area.rs\" \"$ROOT/apps/cli/src/tui/render/input/input_area\" \"$ROOT/apps/cli/src/tui/render/status\" \"$ROOT/apps/cli/src/tui/render/output_area.rs\" \"$ROOT/apps/cli/src/tui/render/output_area\" \"$ROOT/apps/cli/src/tui/render/display/status_bar_selection.rs\" --include='*.rs' | grep -vE 'vm:\\s*&'; perl -ne 'if (/pub\\(super\\)[[:space:]]+(text|cursor):/) { print \"\$ARGV:\$.:\$_\" }' \"$ROOT/apps/cli/src/tui/render/input/input_area.rs\" \"$ROOT/apps/cli/src/tui/render/input/input_area/editing.rs\"; grep -RInE '^[[:space:]]*pub\\(super\\)[[:space:]]+(focused|pending_images|content_width):' \"$ROOT/apps/cli/src/tui/render/input/input_area.rs\" \"$ROOT/apps/cli/src/tui/render/input/input_area\" --include='*.rs'; grep -RInE '^[[:space:]]*(pub(\\([^)]*\\))?[[:space:]]+)?(last_visible_height|last_line_count|scroll_offset|auto_scroll)[[:space:]]*:' \"$ROOT/apps/cli/src/tui/render/output_area.rs\" \"$ROOT/apps/cli/src/tui/render/output_area\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/render/output_area/render.rs:' || true" # guard-registry:scope.tui.arch.inline-exclusions
  report_matches \
    "TUI render widgets must not restore completion/suggestions or spinner mirror storage/types." \
    grep -RInE '(pub\(super\)[[:space:]]+suggestions:[[:space:]]*Vec|pub[[:space:]]+selected_suggestion|pub[[:space:]]+show_suggestions|\bstruct[[:space:]]+SpinnerState\b|\bpub[[:space:]]+struct[[:space:]]+SpinnerState\b)' \
      "$ROOT/apps/cli/src/tui/render/input" "$ROOT/apps/cli/src/tui/render/output_area" --include='*.rs'

  report_matches \
    "TUI render widgets must not expose production state mutation/readback APIs; route through model/view_state and projection helpers." \
    bash -c "perl -ne 'BEGIN { \$pending=0 } if (/^\\s*#\\[cfg\\(test\\)\\]/ || /^\\s*mod tests/) { \$pending=1; next } if (/pub[[:space:]]*(\\([^)]*\\))?[[:space:]]*fn[[:space:]]+(set_text|set_cursor_byte_index|text_snapshot|get_text|add_history|reset_history_nav|navigate_history|set_history|history_previous|history_next|set_pending_images|set_focused|handle_resize|set_success|set_warning|reset_runtime_state|set_thinking|apply_runtime_view|init|set_model|set_session_id|set_tps|set_tokens|set_api_calls|set_context_size|set_context_paths|set_git_context|clear_selection|get_selected_text|start_selection|start_selection_at|update_selection|update_selection_at|end_selection|select_word|set_selection_for_test|set_suggestions|clear_suggestions|set_selected_suggestion|selected_suggestion|is_showing_suggestions|accept_suggestion|select_previous|select_next)[[:space:]]*\\(/ && !\$pending) { print \"\$ARGV:\$.:\$_\" } \$pending=0' \"$ROOT/apps/cli/src/tui/render/input/input_area.rs\" \"$ROOT/apps/cli/src/tui/render/input/input_area/editing.rs\" \"$ROOT/apps/cli/src/tui/render/input/input_area/history.rs\" \"$ROOT/apps/cli/src/tui/render/input/input_area/resize.rs\" \"$ROOT/apps/cli/src/tui/render/input/input_area/selection.rs\" \"$ROOT/apps/cli/src/tui/render/status/bar.rs\" \"$ROOT/apps/cli/src/tui/render/display/status_bar_selection.rs\" \"$ROOT/apps/cli/src/tui/render/output_area/selection.rs\" \"$ROOT/apps/cli/src/tui/render/output_area/render.rs\""

  report_matches \
    "TUI production paths must not drive or read widget mirrors as truth; mutate model/view_state instead." \
    grep -RInE '\b(input_area|status_bar|output_area)\.(set_text|set_cursor_byte_index|set_pending_images|get_text|cursor_position|is_empty|is_showing_suggestions|selected_suggestion|get_selected_text|start_selection|start_selection_at|update_selection|update_selection_at|end_selection|scroll_up|scroll_down|scroll_to_bottom|scroll_to_top|start_spinner|stop_spinner|set_spinner_phase|tick_spinner|set_task_status)\(' \
      "$ROOT/apps/cli/src/tui" --include='*.rs' --exclude='*_tests.rs' # guard-registry:scope.tui.arch.inline-exclusions

  report_matches \
    "TUI production paths must not write widget mirror fields directly; write model/view_state state instead." \
    bash -c "grep -RInE '\\b(input_area|status_bar|output_area|output|self)\\.(scroll_offset|auto_scroll|is_selecting|selection_start|selection_end|selection_row|selection_width|spinner|task_status_lines|queued_submission_lines)\\s*=' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/view_state/' | grep -v 'view_state\\.' | grep -v '/render/output_area' | grep -v '/render/input/input_area/selection.rs' | grep -v '/render/display/status_bar_selection.rs'" # guard-registry:scope.tui.arch.inline-exclusions
  report_matches \
    "OutputArea selection/copy coordinate helpers must remain read-only pure projection helpers." \
    bash -c "perl -0ne 'while (/pub[[:space:]]+fn[[:space:]]+(get_line_content|screen_to_anchor|word_bounds_at|selected_text_for_view)[^{;]*&mut[[:space:]]+self/sg) { print \"\$ARGV:\$1 requires &mut self\\n\" } while (/fn[[:space:]]+selected_text_for_range[^{;]*&mut[[:space:]]+self/sg) { print \"\$ARGV:selected_text_for_range requires &mut self\\n\" }' \"$ROOT/apps/cli/src/tui/render/output_area/selection.rs\""

  report_matches \
    "TUI output document projection must stay centralized; render widgets must not own renderer cache or use legacy widget refresh APIs." \
    bash -c "grep -RInE 'handle_resize\\([^)]*visible_height|visible_height_hint|output_area\\.last_visible_height|pub[[:space:]]+document_renderer|[[:space:]]document_renderer[[:space:]]*:|refresh_output_widget_from_model' \"$ROOT/apps/cli/src/tui\" --include='*.rs'; grep -RInE 'output_area\\.replace_document\\(|\\barea\\.replace_document\\(|\\boutput\\.replace_document\\(' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='*_tests.rs' | grep -v '/app/update.rs:' | grep -v '/render/output_area.rs:' | grep -v '/render/output_area/render.rs:' | grep -v '/render/output_area/selection.rs:' | grep -v '/render/output/selection_tests.rs:' | grep -v '/adapter/output_widget.rs:'; grep -RInE '\\.[[:space:]]*set_document[[:space:]]*\\(' \"$ROOT/apps/cli/src/tui\" --include='*.rs' || true" # guard-registry:scope.tui.arch.inline-exclusions
  report_matches \
    "queued live-status lines must not be read as business truth from OutputArea; use ConversationModel.queued_submissions / LiveStatusViewModel." \
    bash -c "grep -RInE 'queued_submission_lines' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='*_tests.rs' | grep -v '^[^:]*:[0-9][0-9]*:[[:space:]]*//' | grep -v '/app/update/notice.rs:'" # guard-registry:scope.tui.arch.inline-exclusions
  report_matches \
    "model.input.document mutations outside InputModel are forbidden; use InputIntent -> InputModel::apply." \
    grep -RInE 'model\.input\.document\.(clear\(|insert_text\(|replace_text\(|move_|set_cursor_col|delete_)' \
      "$ROOT/apps/cli/src/tui" --include='*.rs' --exclude-dir='model/input' # guard-registry:scope.tui.arch.inline-exclusions

  report_matches \
    "ChatState must not mirror token/api/thinking usage; keep usage/thinking in RuntimeModel and derive status via StatusViewAssembler." \
    grep -RInE '(total_input_tokens|total_output_tokens|total_api_calls|last_input_tokens|usage_snapshot|record_usage|thinking_enabled)' \
      "$ROOT/apps/cli/src/tui/app/state" --include='*.rs'

  return "$fail"
}

echo "[hook-env] AEMEATH_PROJECT_DIR=${AEMEATH_PROJECT_DIR:-<unset>}"
echo "[hook-env] CLAUDE_PROJECT_DIR=${CLAUDE_PROJECT_DIR:-<unset>}"
echo "[hook-env] ROOT=$ROOT"
"$HOOKS_DIR/check-guard-registry.sh"
"$HOOKS_DIR/check-cargo-dependency-graph.sh"
"$HOOKS_DIR/check-cli-thin-entry.sh"
"$HOOKS_DIR/check-share-no-upstream-deps.sh"
"$HOOKS_DIR/check-share-minimal-kernel.sh"
"$HOOKS_DIR/check-composition-layout.sh"
"$HOOKS_DIR/check-cola-layer-purity.sh"
"$HOOKS_DIR/check-crate-api-boundary.sh"
"$HOOKS_DIR/check-task-persistence-capability.sh"
"$HOOKS_DIR/check-provider-invocation-scope.sh"
"$HOOKS_DIR/check-provider-pull-stream.sh"
"$HOOKS_DIR/check-provider-http-attempt.sh"
"$HOOKS_DIR/check-provider-retry-ownership.sh"
"$HOOKS_DIR/check-provider-usage-capability.sh"
"$HOOKS_DIR/check-provider-driver-acl.sh"
"$HOOKS_DIR/check-context-architecture.sh"
"$HOOKS_DIR/check-forbidden-imports.sh"
"$HOOKS_DIR/check-tui-tea-purity.sh"
"$HOOKS_DIR/check-tui-toplevel-layout.sh"
"$HOOKS_DIR/check-tui-effect-boundary.sh"
"$HOOKS_DIR/check-tui-model-view-boundaries.sh"
run_tui_single_source_structure_guard
"$HOOKS_DIR/check-tui-output-legacy-guards.sh"
"$HOOKS_DIR/check-tui-block-nesting.sh"
"$HOOKS_DIR/check-render-pure.sh"
"$HOOKS_DIR/check-render-isolation.sh"
"$HOOKS_DIR/check-unsafe-text-ops.sh"
"$HOOKS_DIR/check-log-target-prefix.sh"
"$HOOKS_DIR/check-logging-scope-context.sh"
"$HOOKS_DIR/check-logging-settings-injection.sh"
"$HOOKS_DIR/no_mod_rs.sh"
"$HOOKS_DIR/check-config-env-guard.sh"
"$HOOKS_DIR/check-agent-client-trait-minimal.sh"
"$HOOKS_DIR/check-shared-run-loop.sh"
"$HOOKS_DIR/check-run-control-boundary.sh"
"$HOOKS_DIR/check-config-reader-injection.sh"
"$HOOKS_DIR/check-config-workflow-boundary.sh"
"$HOOKS_DIR/check-production-reachability.sh"

echo "All architecture guards passed."
