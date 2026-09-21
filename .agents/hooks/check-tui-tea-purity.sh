#!/bin/bash
set -euo pipefail
# guard-registry:scope.tui.tea-runtime-files

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
export ROOT
FAILED=0
COUNT=0

TUI_PURE_DIRS=(
  "apps/cli/src/tui/app"
  "apps/cli/src/tui/model"
  "apps/cli/src/tui/view_assembler"
  "apps/cli/src/tui/view_model"
)

# ---------------------------------------------------------------------------
# 豁免名单（EXEMPT）：tui/app/ 下属于 runtime / 命令执行层、预期含副作用
# （async、block_on、spawn、Command 等）的文件。严格 TEA 纯度检查仍作用于
# update/ 与 state/ 子目录、slash 分发以及纯数据模块（event.rs、msg.rs、resize.rs）。
#
# 各项豁免理由（#59 S5-gap 裁定）：
#   mod.rs              — 同步 git 元数据探测（Command::new），非 update 副作用。
#   run_loop.rs         — runtime 编排层（事件循环 .await），TEA 副作用执行器所在。
#   runtime.rs          — runtime 编排层 / Effect executor 本身，.await 为其职责。
#
# 注：slash 分发已纯化为同步 update（返回 Effect，#947），连同其测试文件
# 一并移出本名单；A1-A4 已 Effect 化/转纯的文件（dialog.rs、suggestions.rs、
# 已删除的 save.rs、memory.rs）同样受严格纯度检查约束。
# ---------------------------------------------------------------------------
# guard-registry:scope.tui.tea-runtime-files
EXEMPT_FILES=(
  "apps/cli/src/tui/app/mod.rs"
  "apps/cli/src/tui/app/run_loop.rs"
  "apps/cli/src/tui/app/runtime.rs"
)

is_exempt() {
  local rel="$1"
  local f
  for f in "${EXEMPT_FILES[@]}"; do
    if [[ "$rel" == "$f" ]]; then
      return 0
    fi
  done
  return 1
}

for dir in "${TUI_PURE_DIRS[@]}"; do
  TARGET="$ROOT/$dir"
  if [[ ! -d "$TARGET" ]]; then
    continue
  fi

  while IFS=: read -r rel line_no line; do
      # Skip files in the exemption list (runtime / command-execution layer)
      if is_exempt "$rel"; then
        continue
      fi

      # guard-registry:false-positive.tui.tea-inline-allow
      if [[ "$line" == *"allow tea_side_effect"* ]]; then
        continue
      fi
      printf 'TUI update side effect: %s:%s:%s\n' "$rel" "$line_no" "$line"
      FAILED=1
      COUNT=$((COUNT + 1))
    done < <(
      # perl 单进程批量处理全部文件（$ARGV 携带文件名），避免逐文件 fork perl
      find "$TARGET" -name '*.rs' -print0 | xargs -0 perl -ne '
        my $rel = $ARGV;
        $rel =~ s/^\Q$ENV{ROOT}\E\///;
        if (/tokio::spawn\s*\(/ || /std::thread::spawn\s*\(/ || /Command::new\s*\(/ || /HookRunner::run|\.run_hook\s*\(/ || /clipboard::|arboard::|copypasta::/ || /read_clipboard_image\s*\(/ || /process_image_file\s*\(/ || /\bHandle::block_on\s*\(|\bRuntime::block_on\s*\(/ || /block_in_place\b/ || /\.await\b/) {
          print "$rel:$.:$_";
        }
if (eof) { close ARGV; }
                    '
    )
done

if [[ "$FAILED" -ne 0 ]]; then
  echo "TUI update side effects found ($COUNT). Return Cmd variants from update() and execute side effects in app runtime/cmd_exec instead."
  exit 1
fi

echo "TUI update TEA purity OK."
