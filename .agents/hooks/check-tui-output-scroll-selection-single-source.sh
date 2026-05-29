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

# OutputArea 的 scroll_offset / auto_scroll / is_selecting / selection_start / selection_end
# 是滚动/选区真相 view_state.output（OutputViewState）的运行态镜像。唯一生产写入路径为
# adapter/output_view_widget.rs（apply_output_scroll_to_widget + apply_output_selection_to_widget），
# 每帧由 app/update.rs 装配后写回 widget。任何 update/effect/slash/render 业务路径都不得直接改
# output(_area).<mirror>，也不得调用已删除的 widget 滚动方法（防回归再添加）。
#
# 豁免：
#  - view_state/ 目录：滚动/选区真相 OutputViewState，self 即真相所在；其 receiver 为 view_state，
#    通过 perl 负向过滤排除 `view_state.output.<field>`（避免误伤 `app.view_state.output.x =`）。
#  - render/output_area/ 目录：OutputArea 自身内部方法（reset_runtime_state 清镜像、
#    clear_selection 等保留的合法方法，及 #[cfg(test)] 测试脚手架）。
#  - render/input_area、render/display 目录：InputArea / StatusBar 等其它 widget 复用同名字段，
#    与 OutputArea 镜像无关。
#  - adapter/output_view_widget.rs：唯一镜像写回路径。
#  - *_tests.rs：测试文件按 spec 允许直填镜像 + set_selection_for_test。
#  - 其余生产文件里的 #[cfg(test)] mod 测试块由 scan_nontest 剥离，业务代码仍受守卫。
#
# 注意 receiver 区分：正则只匹配 widget receiver（output. / output_area. / self.），
# 真相 view_state.output.<field> 的 receiver 是 view_state，被 perl 负向过滤排除。
# 即 self.scroll_offset / output.scroll_offset / output_area.scroll_offset 命中，
# 而 view_state.output.scroll_offset 不命中。

# 逐文件剥离 #[cfg(test)] mod {...} 块（含整文件测试模块），输出 "file:lineno:content"。
# 这样嵌在生产文件里的测试代码也被豁免，而业务代码仍受守卫。
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

# 列出需扫描的文件（排除真相 / widget 内部 / 其它 widget / 写回适配器 / *_tests.rs）。
# 兼容 macOS 自带 bash 3.2：不使用 mapfile，改用换行分隔（.rs 路径不含换行）。
list_scan_files() {
  find "$ROOT/apps/cli/src/tui" -type f -name '*.rs' \
    | grep -vE '/view_state/|/output_area/|/input_area/|/display/' \
    | grep -vE '/output_view_widget\.rs$' \
    | grep -vE '_tests\.rs$'
}

# 1) 直写 OutputArea 镜像字段（receiver 限定为 output / output_area / self）。
rule1_scan() {
  local f
  list_scan_files | while IFS= read -r f; do
    scan_nontest "$f"
  done \
    | grep -E '\b(output|output_area|self)\.(scroll_offset|auto_scroll|is_selecting|selection_start|selection_end)\s*=' \
    | perl -ne 'print unless /(?<![\w.])view_state\.output\./'
}

report_matches \
  "output(_area) 的 scroll_offset/auto_scroll/is_selecting/selection_start/selection_end 镜像只能由 adapter/output_view_widget.rs 写回（以及 OutputArea 自身 reset/clear_selection 与测试代码）；请改 view_state.output 真相，再由装配器写回 widget。" \
  rule1_scan

# 2) 调用已删除 / 应禁的 OutputArea widget 滚动方法（防回归再添加）。
#    选区 widget 方法 start/update/select_word/end_selection 已删，clear_selection /
#    screen_to_anchor / get_selected_text 是保留的合法只读/跨区方法，不在此禁列。
report_matches \
  "output_area.scroll_up/scroll_down/scroll_to_bottom/scroll_to_top 已从 widget 删除；滚动请走 OutputIntent + view_state.output + 写回适配器，不要在 widget 上重新添加这些方法。" \
  grep -RInE 'output_area\.(scroll_up|scroll_down|scroll_to_bottom|scroll_to_top)\(' \
    "$ROOT/apps/cli/src/tui" --include='*.rs' \
    --exclude='*_tests.rs'

exit "$fail"
