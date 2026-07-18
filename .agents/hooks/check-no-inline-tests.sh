#!/usr/bin/env bash
# 检查源码文件中是否存在内嵌 #[cfg(test)] mod tests。
#
# 约定（specs/rust-coding.md）：测试文件 MUST 与源码分离（foo.rs ↔ foo_tests.rs），
# 通过 #[cfg(test)] #[path = "foo_tests.rs"] mod tests; 引入。
# 内嵌 #[cfg(test)] mod tests { ... } 让测试代码不参与 dead code 分析，
# 无法通过"移除测试文件后 cargo build 看 unused warning"发现只在测试中引用的代码。
#
# 允许的分离模式（不视为违规）：
#   #[cfg(test)]
#   #[path = "xxx_tests.rs"]
#   mod tests;
#
# 违规模式：
#   #[cfg(test)]
#   mod tests { ... }   ← 内嵌测试块

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

fail=0

# 扫描所有 .rs 文件，查找内嵌 #[cfg(test)] mod tests {
# （不含 #[path = ...] 的 #[cfg(test)] mod 块）
while IFS= read -r -d '' file; do
  # 跳过测试文件本身
  case "$file" in
    *_tests.rs|*_test.rs) continue ;;
  esac

  # 查找 #[cfg(test)] 后紧跟 mod tests { （中间可有属性如 #[path]，但 #[path] 模式合法）
  # 违规：#[cfg(test)] 后直接 mod xxx { 带花括号体
  violations=$(perl -0777 -ne '
    while (/#\[cfg\(test\)\]\s*(?:#\[path\s*=\s*"[^"]+"\s*\]\s*)?mod\s+(\w+)\s*(\{|;) /sg) {
      my ($name, $term) = ($1, $2);
      # 只有带 { 的内嵌 mod 才违规；带 ; 的是分离文件引入（合法）
      if ($term eq "{") {
        print "$ARGV: 内嵌 #[cfg(test)] mod $name { ... } — 应分离到 ${name}.rs / *_tests.rs\n";
      }
    }
  ' "$file" 2>/dev/null || true)

  if [ -n "$violations" ]; then
    echo "$violations" >&2
    fail=1
  fi
done < <(find "$ROOT/agent" "$ROOT/apps" "$ROOT/packages" -name '*.rs' -print0 2>/dev/null)

if [ "$fail" -ne 0 ]; then
  echo "[architecture] 发现内嵌 #[cfg(test)] mod tests；测试文件 MUST 与源码分离（foo.rs ↔ foo_tests.rs）。" >&2
  echo "  分离后通过 #[cfg(test)] #[path = \"foo_tests.rs\"] mod tests; 引入。" >&2
  exit 1
fi

echo "[check-no-inline-tests] no inline #[cfg(test)] mod tests found."
