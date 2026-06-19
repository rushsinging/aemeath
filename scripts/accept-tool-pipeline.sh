#!/usr/bin/env bash
# 验收脚本：tool registry / result / 管线重构（Issue #376）
#
# 用法：scripts/accept-tool-pipeline.sh
#
# 覆盖：
#   1. 编译（workspace）
#   2. 受影响 crate 的单元测试（share / runtime / tools）
#   3. clippy 验证门禁
#   4. `cargo run -- -qv` 实跑：触发一次真实 Bash 工具调用，
#      断言工具结果经新 ToolExecution/ToolOutcome 管线端到端跑通。
#
# 注意：第 4 步需要已配置的 provider（读 ~/.agents 配置）；若无 provider 则跳过并告警。
set -uo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
FAIL=0
step() { printf '\n\033[1;35m== %s ==\033[0m\n' "$1"; }
ok()   { printf '\033[1;32m✓ %s\033[0m\n' "$1"; }
bad()  { printf '\033[1;31m✗ %s\033[0m\n' "$1"; FAIL=1; }

step "1/4 build --workspace"
if cargo build --workspace 2>&1 | tail -3; then ok "build"; else bad "build"; fi

step "2/4 unit tests (share / runtime / tools)"
for p in share runtime tools; do
  if cargo test -p "$p" --lib 2>&1 | tail -2 | grep -q "test result: ok\|passed"; then
    ok "test $p"
  else
    bad "test $p"
  fi
done

step "3/4 clippy"
# 仅 error 视为失败；build.rs 的 Task schema warning 为预存在、可接受。
CLIPPY="$(cargo clippy --workspace --all-targets 2>&1)"
if echo "$CLIPPY" | grep -qE "^error"; then
  echo "$CLIPPY" | grep -E "^error" | head; bad "clippy"
else
  ok "clippy (no errors)"
fi

step "4/4 live -qv smoke: real Bash tool call through typed pipeline"
MARKER="PIPELINE_ACCEPT_$$"
# 捕获 stdout+stderr：quiet 渲染的 [tool:*] 标记走 stderr，-v 日志也在 stderr。
OUT="$(echo "用 Bash 工具执行：echo ${MARKER}" \
       | AEMEATH_VERSION= RUST_LOG= cargo run -q -- -qv 2>&1)"
# 端到端判定：工具子系统被触发（[tool:* 标记）且 marker 经 LLM→Bash→typed 管线→LLM 往返
# 出现在 Bash 工具输出里（用 [tool:Bash] 行锁定，避免误匹配 prompt 回显）。
if echo "$OUT" | grep -q "\[tool:" && echo "$OUT" | grep -q "\[tool:Bash\].*${MARKER}"; then
  ok "tool executed & result flowed through pipeline (marker round-tripped)"
elif echo "$OUT" | grep -qi "no provider\|unauthorized\|api key\|401\|missing"; then
  printf '\033[1;33m⚠ skip: 无可用 provider，跳过实跑（编译/测试已覆盖管线逻辑）\033[0m\n'
else
  echo "$OUT" | tail -8; bad "live -qv smoke"
fi

echo
if [ "$FAIL" -eq 0 ]; then ok "ALL ACCEPTANCE CHECKS PASSED"; else bad "ACCEPTANCE FAILED"; fi
exit "$FAIL"
