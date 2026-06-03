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

# Input text/cursor truth lives in model.input.document. Completion/suggestions truth lives in
# model.input.completion. Production paths send InputIntent -> InputModel::apply and project
# InputChange through adapter/input_widget.rs into InputArea, which is only a textarea/render mirror.

report_matches \
  "InputArea production text/cursor mutation methods must stay crate-private; only adapter/input_widget.rs may project InputChange into the widget mirror." \
  bash -c "perl -ne 'BEGIN { \$pending=0 } if (/^\\s*#\\[cfg\\(test\\)\\]/) { \$pending=1; next } if (/pub[[:space:]]+fn[[:space:]]+(set_text|set_cursor_byte_index|clear|set_pending_images|get_text)[[:space:]]*\\(/ && !\$pending) { print \"\$ARGV:\$.:\$_\" } \$pending=0' \"$ROOT/apps/cli/src/tui/render/input/input_area.rs\" \"$ROOT/apps/cli/src/tui/render/input/input_area/editing.rs\""

report_matches \
  "production app/update code must not drive InputArea text/cursor directly; send InputIntent and project InputChange via adapter/input_widget.rs." \
  grep -RInE '\binput_area\.(set_text|set_cursor_byte_index|clear|set_pending_images|get_text|cursor_position|is_empty)\(' \
    "$ROOT/apps/cli/src/tui/app" "$ROOT/apps/cli/src/tui/input" --include='*.rs'

report_matches \
  "model.input.document mutations outside InputModel are forbidden; use InputIntent -> InputModel::apply." \
  grep -RInE 'model\.input\.document\.(clear\(|insert_text\(|replace_text\(|move_|set_cursor_col|delete_)' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude-dir='model/input'

report_matches \
  "InputArea must not regain completion/suggestions storage or mutation APIs; keep suggestions derived from model.input.completion." \
  grep -RInE '(pub\(super\)[[:space:]]+suggestions:[[:space:]]*Vec|pub[[:space:]]+selected_suggestion|pub[[:space:]]+show_suggestions|fn[[:space:]]+(set_suggestions|clear_suggestions|set_selected_suggestion|selected_suggestion|is_showing_suggestions|accept_suggestion|select_previous|select_next)[[:space:]]*\()' \
    "$ROOT/apps/cli/src/tui/render/input" --include='*.rs'

report_matches \
  "app/update should read completion visibility from model.input.completion, not from InputArea." \
  grep -RInE 'input_area\.(is_showing_suggestions|selected_suggestion)\(' \
    "$ROOT/apps/cli/src/tui/app" --include='*.rs'

exit "$fail"
