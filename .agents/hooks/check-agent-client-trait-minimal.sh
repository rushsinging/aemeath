#!/usr/bin/env bash
set -euo pipefail

# 功能：检查 AgentClient trait 只有 chat() 方法。
# 作用：守住 #567 事件流收口——所有 TUI↔runtime 交互走 ChatInputEvent/ChatEvent，
#       不允许在 trait 上新增 RPC 方法（绕过事件流）。
# 例外：无。

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
ALLOWED = {"chat"}

violations = [m for m in methods if m not in ALLOWED]

if violations:
    import json
    print(json.dumps({
        "decision": "block",
        "reason": (
            f"AgentClient trait 只能有 chat()，不允许新增 RPC 方法。\n"
            f"新交互请走 ChatInputEvent 事件流。\n"
            f"违规方法: {violations}\n"
            f"文件: {path}"
        )
    }, ensure_ascii=False))
    sys.exit(2)

# sanity check: chat 必须存在
if "chat" not in methods:
    print("ERROR: AgentClient trait 缺少 chat() 方法")
    sys.exit(2)

print("AgentClient trait minimal guard OK.")
PY
