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

# #59 S4：input/status 两区的选区真相迁入 view_state（InputSelectionViewState /
# StatusSelectionViewState）。InputArea / StatusBar widget 上的
# is_selecting / selection_start / selection_end / selection_row / selection_width
# 降为只读镜像，唯一生产写回路径为：
#   - adapter/input_widget.rs::apply_input_selection_to_widget
#   - adapter/status_widget.rs::apply_status_selection_to_widget
# 二者均经 widget 的 apply_selection_mirror setter 单向写回，每帧由 app/update.rs 装配。
# 任何 update/effect/slash/render/mouse 业务路径都不得直接改 input_area/status_bar.<mirror>，
# 也不得调用已删除的 widget 选区状态方法（start_selection*/update_selection*/end_selection，
# 防回归再添加）。
#
# 与 check-tui-output-scroll-selection-single-source.sh 的职责划分：
#  - output 区（OutputArea）的同名选区镜像字段由 S2 output guard 专管；本 guard 通过
#    排除 render/output_area/ 目录与该 guard 不重复打架（output 字段写入只在 output_area/ 与
#    其 adapter/output_view_widget.rs，后者已是 S2 豁免）。
#  - 本 guard 专管 input / status 两区 widget 镜像。
#
# 豁免：
#  - view_state/ 目录：InputSelectionViewState / StatusSelectionViewState（及 output）选区真相，
#    self 即真相所在；其 receiver 在 view_state，业务侧 `view_state.input_sel.<field> =` /
#    `view_state.status_sel.<field> =` 是合法真相写入，receiver 非 input_area/status_bar/self，
#    天然不命中 rule1 正则。
#  - render/input/input_area/selection.rs：InputArea 自身 clear_selection（保留）+
#    apply_selection_mirror（adapter 调用的 setter）+ #[cfg(test)]。
#  - render/display/status_bar_selection.rs：StatusBar 自身 clear_selection（保留）+
#    apply_selection_mirror（adapter 调用的 setter）。
#  - render/output_area/ 目录：OutputArea 自身镜像，归 S2 output guard。
#  - adapter/input_widget.rs、adapter/status_widget.rs：唯一镜像写回路径。
#  - *_tests.rs 与生产文件内的 #[cfg(test)] mod 块（由 scan_nontest 剥离）。

# 逐文件剥离 #[cfg(test)] mod {...} 块（含整文件测试模块），输出 "file:lineno:content"。
scan_nontest() {
  perl -ne '
    BEGIN { $intest = 0; $depth = 0; $pending = 0; }
    if (!$intest && !$pending && /^\s*#\[cfg\(test\)\]/) { $pending = 1; next; }
    if ($pending) {
      if (/\bmod\b/) {
        $pending = 0; $intest = 1; $depth = 0;
        $depth += ($_ =~ tr/{//); $depth -= ($_ =~ tr/}//);
        if ($depth <= 0 && /\{/) { $intest = 0; }
        next;
      }
      $pending = 0;
    }
    if ($intest) {
      $depth += ($_ =~ tr/{//); $depth -= ($_ =~ tr/}//);
      if ($depth <= 0) { $intest = 0; }
      next;
    }
    print "$ARGV:$.:$_";
  ' "$1"
}

# 列出需扫描的文件（排除真相 view_state / output_area widget(归 S2) / input&status 镜像 setter /
# 写回适配器 / *_tests.rs）。兼容 macOS 自带 bash 3.2：不使用 mapfile，改用换行分隔。
list_scan_files() {
  find "$ROOT/apps/cli/src/tui" -type f -name '*.rs' \
    | grep -vE '/view_state/' \
    | grep -vE '/output_area/' \
    | grep -vE '/input/input_area/selection\.rs$' \
    | grep -vE '/display/status_bar_selection\.rs$' \
    | grep -vE '/adapter/(input_widget|status_widget)\.rs$' \
    | grep -vE '_tests\.rs$'
}

# 1) 直写 InputArea / StatusBar 选区镜像字段（receiver 限定为 input_area / status_bar / self）。
#    perl 负向过滤排除 `==` 比较（如 status/bar.rs 的 `self.selection_row == ...`）与
#    view_state 真相写入（receiver 为 view_state.*，不在前缀内，已天然排除）。
rule1_scan() {
  local f
  list_scan_files | while IFS= read -r f; do
    scan_nontest "$f"
  done \
    | grep -E '\b(input_area|status_bar|self)\.(is_selecting|selection_start|selection_end|selection_row|selection_width)\s*=' \
    | perl -ne 'print unless /\b(?:input_area|status_bar|self)\.(?:is_selecting|selection_start|selection_end|selection_row|selection_width)\s*==/'
}

report_matches \
  "input_area/status_bar 的 is_selecting/selection_start/selection_end/selection_row/selection_width 镜像只能由 adapter/{input_widget,status_widget}.rs 写回（经各 widget apply_selection_mirror，及 widget 自身 clear_selection）；请改 view_state.input_sel/status_sel 真相，再由装配器写回 widget。" \
  rule1_scan

# 2) 调用已删除 / 应禁的 InputArea / StatusBar widget 选区状态方法（防回归再添加）。
#    clear_selection / get_selected_text / screen_to_* / apply_selection_mirror /
#    spans_with_selection 是保留的合法只读 / clear / 镜像方法，不在此禁列。
report_matches \
  "input_area/status_bar.start_selection/start_selection_at/update_selection/update_selection_at/end_selection 已从 widget 删除；选区请走 view_state.input_sel/status_sel + 写回适配器，不要在 widget 上重新添加这些状态方法。" \
  grep -RInE '\b(input_area|status_bar)\.(start_selection|start_selection_at|update_selection|update_selection_at|end_selection)\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude='*_tests.rs'

exit "$fail"
