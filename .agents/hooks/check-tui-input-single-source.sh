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

# Input text/cursor truth lives in model.input.document; completion/suggestions truth lives in
# model.input.completion. App/update/effect paths must send InputIntent and let InputModel::apply
# produce InputChange. InputArea must remain a textarea/render mirror and must not regain completion
# suggestion storage or public mutation/selection APIs.
report_matches \
  "input_area text/cursor mutations are allowed only in input widget internals or adapter/input_widget.rs; completion/suggestions must be driven by model.input.completion." \
  bash -c "grep -RInE 'input_area\.(set_text|move_left|move_right|move_up|move_down|move_home|move_end|delete_word|backspace|input\(|enter\(|clear\(|history_up|history_down|set_pending_images)' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='input_widget.rs' --exclude='tests.rs' --exclude-dir='input_area' --exclude-dir='model/input' | grep -v 'input_area\\.input(ch)' | grep -v 'input_area\.input(ch)'"

report_matches \
  "InputArea must not regain completion/suggestions storage or mutation APIs; keep suggestions derived from model.input.completion." \
  grep -RInE '(pub\(super\)[[:space:]]+suggestions:[[:space:]]*Vec|pub[[:space:]]+selected_suggestion|pub[[:space:]]+show_suggestions|fn[[:space:]]+(set_suggestions|clear_suggestions|set_selected_suggestion|selected_suggestion|is_showing_suggestions|accept_suggestion|select_previous|select_next)[[:space:]]*\()' \
    "$ROOT/apps/cli/src/tui/render/input" --include='*.rs'

report_matches \
  "model.input.document mutations outside InputModel are forbidden; use InputIntent -> InputModel::apply." \
  grep -RInE 'model\.input\.document\.(clear\(|insert_text\(|replace_text\(|move_|set_cursor_col|delete_)' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude-dir='model/input'

report_matches \
  "app/update should not read input_area text/cursor as business truth; use model.input.document." \
  grep -RInE 'input_area\.(get_text\(|cursor_position\(|is_empty\()' \
    "$ROOT/apps/cli/src/tui/app" "$ROOT/apps/cli/src/tui/input" --include='*.rs' \
    --exclude-dir='input_area'

report_matches \
  "app/update should read completion visibility from model.input.completion, not from InputArea." \
  grep -RInE 'input_area\.(is_showing_suggestions|selected_suggestion)\(' \
    "$ROOT/apps/cli/src/tui/app" --include='*.rs'

exit "$fail"
