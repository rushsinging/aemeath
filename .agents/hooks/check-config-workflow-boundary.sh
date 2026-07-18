#!/usr/bin/env bash
# check-config-workflow-boundary.sh
#
# 守卫：禁止 Workflow graph policy 标识符回到 Config 域。
#
# Workflow 是独立支撑域 BC（#743/#760 决策）。以下 Workflow graph policy
# 标识符禁止出现在 agent/shared/src/config 的生产 Rust 代码中：
#   - ReasoningGraphConfig
#   - reasoning_graph
#   - NodeEffortConfig
#   - ReasoningGraphNodesConfig
#   - max_reasoning
#
# 例外：snapshot.rs 中 retired_reasoning_graph_section_is_ignored_by_config
# 兼容测试验证退役 reasoning_graph 字段被 serde 忽略。该测试位于 #[cfg(test)]
# 模块，不属于生产代码，自动豁免。
#
# 规则无路径白名单：扫描 config 下全部 .rs 生产代码（#[cfg(test)] 之前的非注释行）。
# 违规退出码 2。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

CONFIG_DIR="$ROOT/agent/shared/src/config"

if [ ! -d "$CONFIG_DIR" ]; then
  echo "Config-Workflow boundary guard: config dir not found, skip."
  exit 0
fi

# 禁止的 Workflow graph policy 标识符
FORBIDDEN_PATTERN='ReasoningGraphConfig|reasoning_graph|NodeEffortConfig|ReasoningGraphNodesConfig|max_reasoning'

# 唯一允许的兼容测试字符串（验证退役字段被忽略的向后兼容测试）
ALLOWED_COMPAT_TEST='retired_reasoning_graph_section_is_ignored_by_config'

violations="$(mktemp)"

while IFS= read -r file; do
  rel="${file#$ROOT/}"

  # 跳过独立测试文件
  case "$rel" in
    *_test.rs|*_tests.rs|*tests.rs|*/tests/*) continue ;;
  esac

  # 提取生产代码：#[cfg(test)] 之前的全部行（测试模块位于生产代码之后）
  prod="$(awk '/\[cfg\(test\)\]/{exit}1' "$file" 2>/dev/null)"

  # 在生产代码中搜索禁止标识符；
  # 排除注释行（///? | //!）—守卫针对实际代码/serde 声明，非文档说明；
  # 排除兼容测试字符串。
  echo "$prod" | grep -nE "$FORBIDDEN_PATTERN" 2>/dev/null | \
# guard-registry:scope.config.workflow-comments-and-compat-test
    grep -vE '^[0-9]+:[[:space:]]*(///?|//!)' | \
# guard-registry:scope.config.workflow-comments-and-compat-test
    grep -v "$ALLOWED_COMPAT_TEST" | \
    sed "s|^|$rel:|" >>"$violations" || true
done < <(find "$CONFIG_DIR" -name '*.rs' -type f | sort)

if [ -s "$violations" ]; then
  echo "Config-Workflow boundary guard FAILED:" >&2
  echo "  Workflow graph policy 禁止回到 Config 域。" >&2
  echo "" >&2
  echo "  违规位置 (file:line:content):" >&2
  sed 's/^/    /' "$violations" >&2
  echo "" >&2
  echo "  Workflow 是独立支撑域 BC，reasoning_graph / max_reasoning 等" >&2
  echo "  graph policy 属于 Workflow 域，不得出现在 config 生产代码中。" >&2
  echo "  参考 #743/#760 BC 锁定决策。" >&2
  rm -f "$violations"
  exit 2
fi

rm -f "$violations"
echo "Config-Workflow boundary guard OK."
