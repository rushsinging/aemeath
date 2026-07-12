#!/usr/bin/env bash
set -euo pipefail

# 功能：检查 AgentClient trait 只有 chat() + cancel_run() 方法。
# 作用：守住 #567 事件流收口；内容输入与结果回传走 ChatInputEvent/ChatEvent。
#       #700 唯一例外是同步、out-of-band 的 cancel_run(run_id)，用于即时触发 per-Run scope。
# 例外：cancel_run 必须按 RunId 定位，不允许扩展为无标识的会话级取消。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

CLIENT_RS="packages/sdk/src/client.rs"

if [ ! -f "$CLIENT_RS" ]; then
  echo "skip: $CLIENT_RS not found"
  exit 0
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
ALLOWED = {"chat", "cancel_run"}

violations = [m for m in methods if m not in ALLOWED]

if violations:
    import json
    print(json.dumps({
        "decision": "block",
        "reason": (
            f"AgentClient trait 只能有 chat() 与 cancel_run(run_id)，不允许新增其它 RPC 方法。\n"
            f"内容输入与结果回传请走 ChatInputEvent/ChatEvent；取消仅允许同步 per-Run 入口。\n"
            f"违规方法: {violations}\n"
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
