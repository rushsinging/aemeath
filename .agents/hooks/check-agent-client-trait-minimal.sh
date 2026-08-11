#!/bin/bash
set -euo pipefail

# 功能：检查交互 AgentClient 与可寻址 RunControlClient 各自保持最小公开面。
# 作用：守住窄 façade；内容流仍走 ChatInputEvent/ChatEvent，所有控制命令只交换 SDK 纯值。
#       #1440 增加 cancel_current_run：前台取消由 Runtime 原子选择当前 Main Run，交付层不持 RunId。
#       identity-scoped cancel_run_step / terminate_run 只属于独立 RunControlClient；AgentClient 不再混入管理端能力。
# 例外：无标识 cancel_current_run 只表达“当前前台 Main Run”，后台/远程精确控制仍必须按 RunId 定位。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

CLIENT_RS="packages/sdk/src/client.rs"

if [ ! -f "$CLIENT_RS" ]; then
  echo "skip: $CLIENT_RS not found"
  exit 0
fi

if grep -RIn --include='*.rs' 'ChatInputEvent::Cancel' agent packages apps; then
  echo '{"decision":"block","reason":"旧 ChatInputEvent::Cancel 入口禁止恢复；取消必须走 AgentClient::cancel_current_run 或 RunControlClient 的可寻址控制。"}'
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

# 交互 AgentClient 允许的方法
ALLOWED = {
    "chat", "cancel_current_run", "config_view", "update_config",
    "reply_interaction", "cancel_interaction",
}

violations = [m for m in methods if m not in ALLOWED]

if violations:
    import json
    print(json.dumps({
        "decision": "block",
        "reason": (
            f"AgentClient trait 仅允许 chat、current-run control、Runtime-owned interaction commands 与 Config control-plane。\n"
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

control_match = re.search(r'(?:pub\s+)?trait\s+RunControlClient\b', text)
if not control_match:
    print("ERROR: RunControlClient trait not found")
    sys.exit(2)
control_start = text.index('{', control_match.start())
depth = 0
control_end = control_start
for i in range(control_start, len(text)):
    if text[i] == '{':
        depth += 1
    elif text[i] == '}':
        depth -= 1
        if depth == 0:
            control_end = i + 1
            break
control_methods = set(re.findall(r'(?:async\s+)?fn\s+(\w+)', text[control_start:control_end]))
required_control = {"cancel_run_step", "terminate_run"}
if control_methods != required_control:
    print(f"ERROR: RunControlClient methods must exactly equal {sorted(required_control)}, found {sorted(control_methods)}")
    sys.exit(2)

print("AgentClient / RunControlClient minimal guard OK.")
PY
