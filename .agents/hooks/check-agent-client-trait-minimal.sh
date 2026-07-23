#!/usr/bin/env bash
set -euo pipefail

# 功能：检查 AgentClient trait 只开放 chat、Main Run control、Runtime-owned interaction 命令与明确登记的 Config control-plane 方法。
# 作用：守住窄 façade；内容流仍走 ChatInputEvent/ChatEvent，所有控制命令只交换 SDK 纯值。
#       #1247 原子切换 Main cancel_run_step / terminate_run；旧 cancel_run 由 #879 退役。
# 例外：Run control 必须按 RunId 定位，不允许扩展为无标识的会话级取消。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

CLIENT_RS="packages/sdk/src/client.rs"

if [ ! -f "$CLIENT_RS" ]; then
  echo "skip: $CLIENT_RS not found"
  exit 0
fi

if grep -RIn --include='*.rs' 'ChatInputEvent::Cancel' agent packages apps; then
  echo '{"decision":"block","reason":"旧 ChatInputEvent::Cancel 入口禁止恢复；取消必须只走同步 cancel_run(run_id)。"}'
  exit 2
fi

python3 - "$CLIENT_RS" <<'PY'
import re, sys

path = sys.argv[1]
text = open(path).read()

# 找 trait AgentClient 块
m = re.search(r'(?:pub\s+)?trait\s+AgentClient\b', text)
if not m:
    print("skip: AgentClient trait not found")
    sys.exit(0)

# 从 trait 开始位置找匹配的 `{}` 块
start = text.index('{', m.start())
depth = 0
end = start
for i in range(start, len(text)):
    if text[i] == '{':
        depth += 1
    elif text[i] == '}':
        depth -= 1
        if depth == 0:
            end = i + 1
            break

trait_body = text[start:end]

# 找所有 fn 声明（async fn 或 fn）
methods = re.findall(r'(?:async\s+)?fn\s+(\w+)', trait_body)

# 允许的方法
ALLOWED = {
    "chat", "cancel_run", "cancel_run_step", "terminate_run", "config_view", "update_config",
    "reply_interaction", "cancel_interaction",
}

violations = [m for m in methods if m not in ALLOWED]

if violations:
    import json
    print(json.dumps({
        "decision": "block",
        "reason": (
            f"AgentClient trait 仅允许 chat/cancel_run、Runtime-owned interaction commands 与 Config control-plane。\n"
            f"内容输入与结果回传请走 ChatInputEvent/ChatEvent；interaction command 只允许 SDK 纯值。\n"            f"违规方法: {violations}\n"
            f"文件: {path}"
        )
    }, ensure_ascii=False))
    sys.exit(2)

# sanity check: required methods 必须存在
missing = ALLOWED.difference(methods)
if missing:
    print(f"ERROR: AgentClient trait 缺少 required methods: {sorted(missing)}")
    sys.exit(2)

print("AgentClient trait minimal guard OK.")
PY
