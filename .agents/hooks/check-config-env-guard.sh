#!/usr/bin/env bash
# check-config-env-guard.sh
# 禁止 config 包外读取业务 env（AEMEATH_*, *_API_KEY, LLM_*）。
# 业务 env 只允许在以下白名单路径读取：
#   - agent/shared/src/config/adapters/env.rs  (EnvAdapter, 唯一业务 env 读取点)
#   - agent/shared/src/config/adapters/paths.rs (AEMEATH_AGENTS_DIR, 路径根)
#   - packages/global/logging/                (AEMEATH_LOG_LEVEL, 日志层)
#   - build.rs                                (编译期)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

# 业务 env 变量名模式（不包含 AEMEATH_LOG_STDERR — 运行时模式开关，非业务配置）
BUSINESS_ENV_PATTERN='AEMEATH_(CONTEXT_SIZE|PROVIDER|API_KEY|BASE_URL|MODEL|MAX_TOKENS|MAX_REASONING|PERMISSION_MODE|MAX_TOOL_CONCURRENCY|MAX_AGENT_CONCURRENCY|VERBOSE|LOG_LEVEL)|ANTHROPIC_API_KEY|OPENAI_API_KEY|CLAUDE_API_KEY|LLM_API_KEY|LLM_BASE_URL|DEEPSEEK_API_KEY|MINIMAX_API_KEY|MIMO_API_KEY|VOLCENGINE_CODING_PLAN_API_KEY|AGNES_API_KEY|OLLAMA_API_KEY'

# guard-registry:policy.config.business-env-owner
# 唯一 Target 实现点（结构化 policy，非 migration exception）。
WHITELIST_PATTERNS=(
  'agent/features/config/src/adapters.rs'
)

# 扫描 workspace 业务代码。
SCAN_DIRS=(
  "$ROOT/agent"
  "$ROOT/apps"
  "$ROOT/packages"
)

ENV_READ_PATTERN='(std::)?env[[:space:]]*::[[:space:]]*var(_os)?[[:space:]]*\(|use[[:space:]]+(std::)?env[[:space:]]*::[[:space:]]*(var|var_os)|(^|[^[:alnum:]_])(var|var_os)[[:space:]]*\('

fail=0
tmp="$(mktemp)"

for dir in "${SCAN_DIRS[@]}"; do
  if [ ! -d "$dir" ]; then
    continue
  fi
  # 扫描 .rs 文件中的 env::var("AEMEATH_*") 等业务 env 读取
  while IFS= read -r file; do
    rel="${file#$ROOT/}"

    # 跳过白名单
    whitelisted=0
    for pattern in "${WHITELIST_PATTERNS[@]}"; do
      if [[ "$rel" == *"$pattern"* ]]; then
        whitelisted=1
        break
      fi
    done
    if [ "$whitelisted" -eq 1 ]; then
      continue
    fi

    # 跳过测试文件
    if [[ "$rel" == *"_test"* ]] || [[ "$rel" == *"tests"* ]]; then
      continue
    fi

    # 搜索业务 env 读取
    # guard-registry:scope.config.comments
    if grep -nE "($BUSINESS_ENV_PATTERN)" "$file" 2>/dev/null | grep -E "$ENV_READ_PATTERN" | grep -v '//' | sed "s|^|$rel:|" >>"$tmp"; then
      :
    fi
  done < <(find "$dir" -name '*.rs' -type f)
done

if [ -s "$tmp" ]; then
  echo "Config env guard FAILED: 业务 env 变量只能在 Config EnvAdapter 唯一实现点读取。" >&2
  echo "" >&2
  echo "以下位置违规读取了业务 env:" >&2
  cat "$tmp" >&2
  echo "" >&2
  echo "请通过 ConfigReader port (config_view / ConfigSnapshot) 获取配置值。" >&2
  fail=1
fi

rm -f "$tmp"

if [ "$fail" -eq 1 ]; then
  exit 2
fi

echo "Config env guard OK."
