#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
# 守卫：如果 AEMEATH_PROJECT_DIR 不包含 .agents/hooks 说明不是项目根目录，
# 回退到 BASH_SOURCE 推导
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

# spinner/task/queued live-status 真相归 RuntimeModel.spinner (active+phase)、
# RuntimeModel.task_status.lines 与 ConversationModel.queued_submissions；动画 frame/verb
# 归 view_state.spinner。OutputArea 必须直接从 LiveStatusViewModel 投影渲染，不能再
# 物理存储 spinner/task_status_lines/queued_submission_lines widget mirror，也不能经
# adapter 写回这些 mirror。

# 1) retired live-status adapter 不进入生产路径。
report_matches \
  "live_status_widget adapter must stay retired and test-only; live status projection is assembled from model/view_state at render time." \
  bash -c "perl -ne 'BEGIN { \$pending=0 } if (/^\\s*#\\[cfg\\(test\\)\\]/) { \$pending=1; next } if (/^\\s*(\\/\\/.*)?\$/) { next } if (/pub[[:space:]]+mod[[:space:]]+live_status_widget[[:space:]]*;/ && !\$pending) { print \"\$ARGV:\$.:\$_\" } \$pending=0' \"$ROOT/apps/cli/src/tui/adapter.rs\""

# 2) OutputArea 不得重新持有 live-status mirror 字段。
report_matches \
  "OutputArea must not physically store live-status mirror fields; render spinner/task/queued directly from LiveStatusViewModel." \
  grep -RInE '^[[:space:]]*pub[[:space:]]+(spinner|task_status_lines|queued_submission_lines):' \
    "$ROOT/apps/cli/src/tui/render/output_area.rs" \
    "$ROOT/apps/cli/src/tui/render/output_area" --include='*.rs'

# 2) 旧 widget mirror 类型必须退役，避免以别名方式恢复 Instant-based 状态。
report_matches \
  "SpinnerState widget mirror type must stay deleted; use SpinnerLineView from LiveStatusViewModel instead." \
  grep -RInE '\bstruct[[:space:]]+SpinnerState\b|\bpub[[:space:]]+struct[[:space:]]+SpinnerState\b' \
    "$ROOT/apps/cli/src/tui/render/output_area" --include='*.rs'

# 3) 不得直写 OutputArea live-status mirror 字段（防止字段恢复后漏网）。
report_matches \
  "output(_area).spinner / .task_status_lines / .queued_submission_lines mirror writes must stay deleted; derive LiveStatusViewModel and pass it to render/selection." \
  grep -RInE '\b(output|output_area|self)\.(spinner|task_status_lines|queued_submission_lines)\s*=' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude='*_tests.rs'

# 4) 写回 adapter 必须退役。
report_matches \
  "apply_live_status_to_widget must stay deleted; live status is projected into OutputArea::render from LiveStatusViewModel." \
  grep -RInE '\bapply_live_status_to_widget\b' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude='*_tests.rs'

# 5) 调用已删除的 spinner/task 镜像方法（防回归）。
report_matches \
  "start_spinner/stop_spinner/set_spinner_phase/tick_spinner/set_task_status were removed; drive spinner/task via RuntimeIntent + LiveStatusViewModel projection instead." \
  grep -RInE '\.(start_spinner|stop_spinner|set_spinner_phase|tick_spinner|set_task_status)\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude='*_tests.rs'

# 6) 排队输入预览只能来自 ConversationModel.queued_submissions，经 live-status assembler 格式化。
report_matches \
  "queued live-status lines must not be read as business truth from OutputArea; use ConversationModel.queued_submissions / LiveStatusViewModel." \
  bash -c "grep -RInE 'queued_submission_lines' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='*_tests.rs' | grep -v '^[^:]*:[0-9][0-9]*:[[:space:]]*//' | grep -v '/app/update/notice.rs:'"

exit "$fail"
