#!/usr/bin/env bash
set -euo pipefail
# guard-registry:migration.tui.tea-slash-dispatch

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
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
# update/ 与 state/ 子目录以及纯数据模块（event.rs、msg.rs、resize.rs）。
#
# 各项豁免理由（#59 S5-gap 裁定）：
#   mod.rs              — 同步 git 元数据探测（Command::new），非 update 副作用。
#   run_loop.rs         — runtime 编排层（事件循环 .await），TEA 副作用执行器所在。
#   runtime.rs          — runtime 编排层 / Effect executor 本身，.await 为其职责。
#   slash.rs            — B 块 wontfix：命令主分发为 request-response + 返回
#                         Option<String> 控制流语义（命令需 IO 返回值做即时同步
#                         UI 反馈与 prompt 注入决策）。Effect 化需把每命令拆成
#                         "发 Effect + UiEvent 回流续接"状态机，引入大量 pending
#                         状态、破坏 Some(prompt) 直返、重写 slash_tests，收益仅
#                         guard 名单少一项、成本高 → 整文件豁免，行级豁免亦不引入
#                         （14 处 .await 散布于 do-not-touch 分发逻辑，徒增噪声）。
#   slash_tests.rs      — 测试 mock。
#   slash_effect_tests.rs — 测试 mock。
#
# 注：A1-A4 已 Effect 化/转纯的文件（dialog.rs、suggestions.rs、已删除的
# save.rs、memory.rs）已移出本名单，受严格纯度检查约束。
# ---------------------------------------------------------------------------
# guard-registry:migration.tui.tea-slash-dispatch
# guard-registry:scope.tui.tea-runtime-files
# guard-registry:scope.tui.tea-test-files
EXEMPT_FILES=(
  "apps/cli/src/tui/app/mod.rs"
  "apps/cli/src/tui/app/run_loop.rs"
  "apps/cli/src/tui/app/runtime.rs"
  "apps/cli/src/tui/app/slash.rs"
  "apps/cli/src/tui/app/slash_tests.rs"
  "apps/cli/src/tui/app/slash_effect_tests.rs"
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

  while IFS= read -r -d '' file; do
    rel="${file#$ROOT/}"

    # Skip files in the exemption list (runtime / command-execution layer)
    if is_exempt "$rel"; then
      continue
    fi

    while IFS=: read -r line_no line; do
      # guard-registry:false-positive.tui.tea-inline-allow
      if [[ "$line" == *"allow tea_side_effect"* ]]; then
        continue
      fi
      printf 'TUI update side effect: %s:%s:%s\n' "$rel" "$line_no" "$line"
      FAILED=1
      COUNT=$((COUNT + 1))
    done < <(
      perl -ne '
        print "$.:$_" if /tokio::spawn\s*\(/;
        print "$.:$_" if /std::thread::spawn\s*\(/;
        print "$.:$_" if /Command::new\s*\(/;
        print "$.:$_" if /HookRunner::run|\.run_hook\s*\(/;
        print "$.:$_" if /clipboard::|arboard::|copypasta::/;
        print "$.:$_" if /read_clipboard_image\s*\(/;
        print "$.:$_" if /process_image_file\s*\(/;
        # ── New patterns ──────────────────────────────────────────────
        print "$.:$_" if /\bHandle::block_on\s*\(|\bRuntime::block_on\s*\(/;
        print "$.:$_" if /block_in_place\b/;
        print "$.:$_" if /\.await\b/;
      ' "$file"
    )
  done < <(find "$TARGET" -name '*.rs' -print0)
done

if [[ "$FAILED" -ne 0 ]]; then
  echo "TUI update side effects found ($COUNT). Return Cmd variants from update() and execute side effects in app runtime/cmd_exec instead."
  exit 1
fi

echo "TUI update TEA purity OK."
