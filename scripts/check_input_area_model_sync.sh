#!/usr/bin/env bash
# 扫描 TUI input_area 直接修改后是否同步了模型
#
# 背景：Bug #77/#78/#79 都是因为直接调用 input_area.input/enter/set_text
# 修改 textarea widget 但没有同步 self.model.input.document，导致下次按键时
# 模型用旧文本 (TextChanged→set_text) 覆盖 textarea 的正确内容。
#
# 修复模式：修改 textarea 后执行
#   let text = self.input_area.get_text();
#   self.model.input.document.clear();
#   self.model.input.document.insert_text(&text);
#
# 本脚本扫描所有直接调用，检查前后 20 行内是否有文档同步。
#
# 用法：
#   ./scripts/check_input_area_model_sync.sh [目录]
#   默认目录: apps/cli/src/tui

set -euo pipefail

DIR="${1:-apps/cli/src/tui}"

echo "=== 扫描 input_area 直接修改 (input/enter/set_text/delete_strategy) ==="
echo

# 统计
total=0
synced=0
unsynced=0

# 找出所有调用点
grep -rn "self\.input_area\.\(input\|enter\|set_text\|delete_strategy\|delete_backward\|backspace\|move_up\|move_down\|delete_word\|delete_forward\|clear\)(" "$DIR" --include="*.rs" \
  | grep -v "^\s*//" \
  | grep -v "mod tests" \
  | grep -v "#\[cfg" \
  | while IFS=: read -r file line content; do

    # 排除纯注释行
    trimmed=$(echo "$content" | sed 's/^[[:space:]]*//')
    if echo "$trimmed" | grep -q "^//"; then continue; fi

    total=$((total + 1))

    # 检查前后 20 行是否有同步
    sync_found=false
    for offset in $(seq -20 20); do
        target=$((line + offset))
        if [ "$target" -lt 1 ]; then continue; fi
        sync_line=$(sed -n "${target}p" "$file" 2>/dev/null || true)
        if echo "$sync_line" | grep -qE "document\.clear|document\.insert_text|model\.input\.document"; then
            sync_found=true
            break
        fi
    done

    if [ "$sync_found" = true ]; then
        synced=$((synced + 1))
        echo "  ✅ $file:$line  $content"
    else
        unsynced=$((unsynced + 1))
        echo "  ❌ $file:$line  $content"
    fi
done

echo
echo "=== 结果：$total 个调用点，$synced 已同步，$unsynced 未同步 ==="
