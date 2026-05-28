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

# Input text/cursor truth lives in model.input.document. App/update/effect paths must send
# InputIntent and let adapter/input_widget.rs apply InputChange to InputArea.
report_matches \
  "input_area text/cursor mutations are allowed only in input widget internals or adapter/input_widget.rs." \
  bash -c "grep -RInE 'input_area\\.(set_text|move_left|move_right|move_up|move_down|move_home|move_end|delete_word|backspace|input\\(|enter\\(|clear\\(|clear_suggestions|set_suggestions|accept_suggestion|history_up|history_down|set_pending_images)' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='input_widget.rs' --exclude='tests.rs' --exclude-dir='input_area' --exclude-dir='model/input' | grep -v 'input_area\\.input(ch)' | grep -v 'input_area\.input(ch)'"

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

exit "$fail"
