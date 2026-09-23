#!/bin/bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
export ROOT
# 扫描全仓库源码（apps / agent / packages）：provider stream、context compact、
# 工具实现等字符串处理同样可能对任意动态文本做裸字节切片，panic 不限于 CLI。
TARGETS=()
for base in "$ROOT/apps" "$ROOT/agent" "$ROOT/packages"; do
  if [[ -d "$base" ]]; then
    while IFS= read -r dir; do
      TARGETS+=("$dir")
    done < <(find "$base" -type d -name src)
  fi
done
if [[ "${#TARGETS[@]}" -eq 0 ]]; then
  echo "ERROR: no src directories found under apps/ agent/ packages/ in $ROOT" >&2
  exit 2
fi

FAILED=0
COUNT=0

while IFS=: read -r rel line_no line; do
      # guard-registry:false-positive.tui.unsafe-text-inline-allow
      if [[ "$line" == *"allow unsafe_text_op"* ]]; then
        continue
      fi
      if [[ "$line" =~ ^[[:space:]]*#\!?\[ ]]; then
        continue
      fi
      # guard-registry:scope.tui.safe-text-owners
      case "$rel" in
        apps/cli/src/tui/render/display/safe_text.rs|apps/cli/src/tui/display/safe_text.rs|apps/cli/src/tui/text.rs)
            continue
            ;;
      esac
      # guard-registry:scope.all.unsafe-text-long-statement-files
      # 长语句文件（rustfmt 折行会移动行尾注释，行级豁免不稳定），
      # 命中切片均为 find/char_indices 偏移或 Vec/字节切片，逐处确认过。
      case "$rel" in
        apps/cli/src/tui/render/output/blocks/tool_result.rs|agent/features/memory/src/domain/reflection.rs|agent/features/provider/src/adapters/ollama/conversion.rs|agent/features/audit/src/adapters/append.rs|packages/global/logging/src/domain/routing_guard.rs)
            continue
            ;;
      esac
      printf 'unsafe text op: %s:%s:%s\n' "$rel" "$line_no" "$line"
      FAILED=1
      COUNT=$((COUNT + 1))
    done < <(
      # perl 单进程批量处理全部文件（$ARGV 携带文件名），避免逐文件 fork perl——
      # 实测从 4.4s 降到 ~0.3s。
      find "${TARGETS[@]}" -name '*.rs' -print0 | xargs -0 perl -ne '
              my $rel = $ARGV;
              $rel =~ s/^\Q$ENV{ROOT}\E\///;
              # 安全写法豁免：
              # - floor_char_boundary / is_char_boundary 已显式保证边界；
              # - [..] 空上限是全量引用（Vec/字节常见，str 同样安全）；
              # - 整行 // 注释（含 ///、//! 文档示例）不算代码。
              next if /allow\s+unsafe_text_op/;
              next if /floor_char_boundary|is_char_boundary/;
              next if /\[\s*\.\.\s*\]/;
              next if /^\s*\/\//;
              # 危险模式：对任意文本做裸字节切片。
              # - A: &ident[..x] / &ident[a..b]（单标识符，保持原始行为）
              # - B: ident[a..b] 双端标识符（保持原始行为）
              # - C: 截断形态，字段路径支持：path[..N / path[..=N /
              #      path[..x.len( / path[..=x.len( / path[..len(
              #      字段访问的显式边界模型（cursor/bounds）不属于截断，放行。
              if (/\.chars\(\)\.nth\(/ || /&\s*[A-Za-z_][A-Za-z0-9_]*\s*\[[^\]]*\.\.[^\]]*\]/ || /[A-Za-z_][A-Za-z0-9_]*\s*\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\.\.\s*[A-Za-z_][A-Za-z0-9_]*\s*\]/ || /(?:&\s*)?[A-Za-z_][A-Za-z0-9_.]*\s*\[\s*\.\.\s*=\s*(?:\d|[A-Za-z_][A-Za-z0-9_.]*\.len\(|len\()/ || /(?:&\s*)?[A-Za-z_][A-Za-z0-9_.]*\s*\[\s*\.\.\s*(?:\d|[A-Za-z_][A-Za-z0-9_.]*\.len\(|len\()/ || /\.split_at\(/) {
                print "$rel:$.:$_";
              }
if (eof) { close ARGV; }
          '
    )

if [[ "$COUNT" -eq 0 ]]; then
  echo "Unsafe CLI text/index operations found (0)."
fi

if [[ "$FAILED" -ne 0 ]]; then
  echo "Unsafe CLI text/index operations found ($COUNT). Use safe helpers: strip_prefix/strip_suffix, get() with Option, floor_char_boundary(), split_at_ascii(). 确属安全写法时加行级豁免注释：Vec/字节切片用 'allow unsafe_text_op: Vec slice'；str::find / char_indices 偏移切片用 'allow unsafe_text_op: find offset'；固定 ASCII 前缀/hex 窗口用 'allow unsafe_text_op: fixed ascii prefix'。NEVER 用「输入是 ASCII」搪塞动态文本。"
  exit 1
fi
