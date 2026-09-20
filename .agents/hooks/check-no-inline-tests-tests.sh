#!/bin/bash
# check-no-inline-tests.sh 的自检：用 fixture 覆盖正则边界与基线语义。
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
GUARD="$ROOT/.agents/hooks/check-no-inline-tests.sh"

if [ ! -x "$GUARD" ]; then
  echo "guard script missing: $GUARD" >&2
  exit 1
fi

fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT
mkdir -p "$fixture_root/apps/cli/src" "$fixture_root/.agents/hooks"

write_baseline() {
  cat > "$fixture_root/.agents/inline-tests-baseline.json" <<JSON
{
  "reason": "fixture",
  "files": [$1]
}
JSON
}

write_inline_module() {
  cat > "$fixture_root/apps/cli/src/module.rs" <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn case() {}
}
RS
}

write_separated_module() {
  cat > "$fixture_root/apps/cli/src/module.rs" <<'RS'
#[cfg(test)]
#[path = "module_tests.rs"]
mod tests;
RS
}

# 1) `{` 不带尾随空格的内嵌块必须被拒绝（旧正则漏检的写法）。
write_baseline ""
write_inline_module
if AEMEATH_PROJECT_DIR="$fixture_root" "$GUARD" >/dev/null 2>&1; then
  echo "guard must reject an inline #[cfg(test)] mod tests block without trailing space" >&2
  exit 1
fi

# 2) 分离式引入（mod tests;）合法，不得误报。
write_baseline ""
write_separated_module
AEMEATH_PROJECT_DIR="$fixture_root" "$GUARD" >/dev/null

# 3) 注释中引用的内联示例不得误报。
write_baseline ""
cat > "$fixture_root/apps/cli/src/module.rs" <<'RS'
// 违规模式示例：#[cfg(test)] mod tests { ... } —— 这里只是注释
fn production() {}
RS
AEMEATH_PROJECT_DIR="$fixture_root" "$GUARD" >/dev/null

# 4) 基线内登记的存量文件放行。
write_baseline '"apps/cli/src/module.rs"'
write_inline_module
AEMEATH_PROJECT_DIR="$fixture_root" "$GUARD" >/dev/null

# 5) 基线中已无违规的失效条目必须报错，强制收缩基线。
write_separated_module
if AEMEATH_PROJECT_DIR="$fixture_root" "$GUARD" >/dev/null 2>&1; then
  echo "guard must reject a stale baseline entry" >&2
  exit 1
fi

# 6) 缺少基线文件必须报错，避免静默退化为无基线放行。
rm -f "$fixture_root/.agents/inline-tests-baseline.json"
if AEMEATH_PROJECT_DIR="$fixture_root" "$GUARD" >/dev/null 2>&1; then
  echo "guard must fail when the baseline file is missing" >&2
  exit 1
fi

echo "[check-no-inline-tests-tests] positive and negative fixtures passed."
