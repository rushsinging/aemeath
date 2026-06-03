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

# spinner / task_status_lines / queued_submission_lines 是 OutputArea 的 live-status 运行态镜像。
# 真相归 RuntimeModel.spinner (active+phase)、RuntimeModel.task_status.lines 与
# ConversationModel.queued_submissions；动画 frame/verb 归 view_state.spinner。
# 唯一生产写入路径为 view_assembler/live_status.rs -> adapter/live_status_widget.rs
# (apply_live_status_to_widget)，每帧由 app/update.rs::refresh_live_status_from_model 调用。
# 任何 update/effect/slash 业务路径都不得直接改 output(_area).spinner /
# .task_status_lines / .queued_submission_lines，也不得调用已删除的
# start_spinner/stop_spinner/set_spinner_phase/tick_spinner/set_task_status。
#
# 豁免：
#  - adapter/live_status_widget.rs：唯一镜像写入路径。
#  - render/output_area/content.rs：OutputArea 自身 reset_runtime_state 清空镜像。
#  - render/output/status_line.rs：镜像直写仅在其 #[cfg(test)] mod 内（测试构造）。
#  - *_tests.rs：测试文件按 spec 允许直填镜像。
#  - view_state.spinner 是动画源（非 OutputArea 镜像），receiver 不在匹配前缀内，天然不命中。

# 1) 直写 OutputArea 镜像字段（receiver 限定为 output / output_area / self，避免误伤 view_state.spinner）。
report_matches \
  "output(_area).spinner / .task_status_lines / .queued_submission_lines mirror writes are allowed only in adapter/live_status_widget.rs (and OutputArea's own reset / test code); send model intents and let the assembler + apply_live_status_to_widget write the widget." \
  grep -RInE '\b(output|output_area|self)\.(spinner|task_status_lines|queued_submission_lines)\s*=' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude='live_status_widget.rs' \
    --exclude='content.rs' \
    --exclude='status_line.rs' \
    --exclude='*_tests.rs'

# 2) 调用已删除的 spinner/task 镜像方法（防回归）。
report_matches \
  "start_spinner/stop_spinner/set_spinner_phase/tick_spinner/set_task_status were removed; drive spinner/task via RuntimeIntent + the live_status assembler/adapter pipeline instead." \
  grep -RInE '\.(start_spinner|stop_spinner|set_spinner_phase|tick_spinner|set_task_status)\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude='live_status_widget.rs' \
    --exclude='*_tests.rs'

# 3) 排队输入预览只能来自 ConversationModel.queued_submissions，经 live-status assembler 格式化。
report_matches \
  "queued_submission_lines must not be read as business truth outside OutputArea rendering/selection; use ConversationModel.queued_submissions." \
  bash -c "grep -RInE 'queued_submission_lines' \"$ROOT/apps/cli/src/tui\" --include='*.rs' --exclude='output_area.rs' --exclude='live_status_widget.rs' --exclude='render.rs' --exclude='content.rs' --exclude='*_tests.rs' | grep -v '^[^:]*:[0-9][0-9]*:[[:space:]]*//' | grep -v '/app/update/notice.rs:'"

exit "$fail"
