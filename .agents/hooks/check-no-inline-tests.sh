#!/bin/bash
# 检查源码文件中是否存在内嵌 #[cfg(test)] mod tests。
#
# 约定（specs/3.2-rust-coding.md）：测试文件 MUST 与源码分离（foo.rs ↔ foo_tests.rs），
# 通过 #[cfg(test)] #[path = "foo_tests.rs"] mod tests; 引入。
# 内嵌 #[cfg(test)] mod tests { ... } 让测试代码不参与 dead code 分析，
# 无法通过"移除测试文件后 cargo build 看 unused warning"发现只在测试中引用的代码。
#
# 允许的分离模式（不视为违规）：
#   #[cfg(test)]
#   #[path = "xxx_tests.rs"]
#   mod tests;
#
# 违规模式（`{` 与 `;` 之后的空白数量不限）：
#   #[cfg(test)]
#   mod tests { ... }   ← 内嵌测试块
#
# 匹配前先剔除整行 `//` 注释，避免注释中引用的示例被误判。
#
# 历史存量走 `.agents/inline-tests-baseline.json`：该文件登记守卫正则失效期间
# 已存在的内嵌测试文件，只拦截**新增**违规；按 specs/3.2.5.3 渐进迁移，
# NEVER 一次性移动全仓历史测试。基线中已无违规的失效条目同样会失败，
# 强制迁移完成时同步收缩基线。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

BASELINE="$ROOT/.agents/inline-tests-baseline.json"
violations_file="$(mktemp)"
trap 'rm -f "$violations_file"' EXIT

# 扫描所有 .rs 文件，收集内嵌 #[cfg(test)] mod xxx { ... } 文件（相对仓库根、去重）。
while IFS= read -r -d '' file; do
  case "$file" in
    *_tests.rs|*_test.rs) continue ;;
  esac

  if perl -0777 -ne '
    my $source = $_;
    $source =~ s{^[ \t]*//[^\n]*$}{}mg;
    if ($source =~ /#\[cfg\(test\)\]\s*(?:#\[path\s*=\s*"[^"]+"\s*\]\s*)?mod\s+\w+\s*\{/s) {
      print "inline\n";
    }
  ' "$file" 2>/dev/null | grep -q .; then
    printf '%s\n' "${file#"$ROOT"/}" >> "$violations_file"
  fi
done < <(find "$ROOT/agent" "$ROOT/apps" "$ROOT/packages" -name '*.rs' -print0 2>/dev/null)

python3 - "$BASELINE" "$violations_file" <<'PY'
import json
import pathlib
import sys

baseline_path = pathlib.Path(sys.argv[1])
violations_path = pathlib.Path(sys.argv[2])

if not baseline_path.is_file():
    print(
        f"[check-no-inline-tests] 缺少基线文件: {baseline_path}",
        file=sys.stderr,
    )
    sys.exit(1)

baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
allowed = set(baseline.get("files", []))
found = {
    line.strip()
    for line in violations_path.read_text(encoding="utf-8").splitlines()
    if line.strip()
}

added = sorted(found - allowed)
stale = sorted(allowed - found)

if added:
    print(
        "[architecture] 发现新增内嵌 #[cfg(test)] mod tests；测试文件 MUST 与源码分离（foo.rs ↔ foo_tests.rs）。",
        file=sys.stderr,
    )
    for path in added:
        print(f"  {path}", file=sys.stderr)
    print(
        '  分离后通过 #[cfg(test)] #[path = "foo_tests.rs"] mod tests; 引入。',
        file=sys.stderr,
    )

if stale:
    print(
        "[architecture] 基线 .agents/inline-tests-baseline.json 存在失效条目（对应文件已无内嵌测试）；请从基线移除，保持存量清单可收敛。",
        file=sys.stderr,
    )
    for path in stale:
        print(f"  {path}", file=sys.stderr)

if added or stale:
    sys.exit(1)

print(
    f"[check-no-inline-tests] no new inline #[cfg(test)] mod tests (legacy baseline files: {len(allowed)})."
)
PY
