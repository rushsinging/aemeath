#!/usr/bin/env bash
set -euo pipefail

# #700：Main/Sub 必须只通过唯一 shared Loop Engine 驱动 Run。
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

ENGINE="agent/features/runtime/src/application/loop_engine/engine.rs"
MAIN="agent/features/runtime/src/application/main_loop/looping/loop_runner.rs"
SUB="agent/features/runtime/src/application/subagent/runner/loop_run.rs"
LAUNCHER="agent/features/runtime/src/application/run_launcher.rs"
MAIN_PORT="agent/features/runtime/src/application/main_loop/looping/main_run_port.rs"
CONTEXT_COORDINATION="agent/features/runtime/src/application/context_coordination.rs"
OLD_FSM="agent/features/runtime/src/application/main_loop/looping/state.rs"

for path in "$ENGINE" "$MAIN" "$MAIN_PORT" "$SUB" "$LAUNCHER" "$CONTEXT_COORDINATION"; do
  if [ ! -f "$path" ]; then
    echo "{\"decision\":\"block\",\"reason\":\"共享 Loop Engine 守卫缺少文件：$path\"}"
    exit 2
  fi
done

if [ -e "$OLD_FSM" ]; then
  echo "{\"decision\":\"block\",\"reason\":\"旧 ChatLoopState FSM 禁止恢复：$OLD_FSM\"}"
  exit 2
fi

engine_defs=$(grep -RInE 'pub[[:space:]]+async[[:space:]]+fn[[:space:]]+run_loop[[:space:]]*<' \
  agent/features/runtime/src/application --include='*.rs' --exclude='*_tests.rs' | wc -l | tr -d ' ') # guard-registry:scope.runtime.shared-loop-tests
if [ "$engine_defs" -ne 1 ]; then
  echo "{\"decision\":\"block\",\"reason\":\"生产代码必须恰有一个泛型共享 run_loop 定义，当前数量：$engine_defs\"}"
  exit 2
fi

# #1280: Main/Sub may call run_launcher::launch / reenter_run_loop instead of
# calling run_loop directly. The launcher itself must call run_loop.
if ! grep -q 'run_loop(' "$LAUNCHER"; then
  echo '{"decision":"block","reason":"RunLauncher 未调用共享 loop_engine::run_loop。"}'
  exit 2
fi

# Main: accept either direct run_loop or via run_launcher.
if ! grep -qE 'run_loop\(&mut run, &cancel, &mut port\)|run_launcher::(launch|reenter_run_loop)' "$MAIN"; then
  echo '{"decision":"block","reason":"Main Run 未调用共享 loop_engine::run_loop 或 RunLauncher。"}'
  exit 2
fi

# Sub: accept either direct shared_run_loop or via run_launcher.
if ! grep -qE 'shared_run_loop\(&mut run, &cancel, &mut self\)\.await|run_launcher::launch' "$SUB"; then
  echo '{"decision":"block","reason":"Sub Run 未调用共享 loop_engine::run_loop 或 RunLauncher。"}'
  exit 2
fi

if grep -RInE 'context::session::|\bChatChain\b|\bChatSegment\b|save_chain|current_chain|frozen_chats|active_summary|SessionProjectionParticipant|projection_start_index|microcompact_(chain|messages)|compact_messages_with_llm' \
    agent/features/runtime/src --include='*.rs' --exclude='*_tests.rs'; then # guard-registry:scope.runtime.shared-loop-tests
  echo '{"decision":"block","reason":"Runtime 生产代码必须只经 Context crate-root Published Language / ContextPort 使用 Session，禁止内部类型、第二投影 backing、save callback 与 legacy compact helper。"}'
  exit 2
fi

if ! grep -q 'append_finalized' "$MAIN_PORT" || ! grep -q 'append_finalized' "$SUB"; then
  echo '{"decision":"block","reason":"Main/Sub execution path 必须各自接入唯一 finalized Step append。"}'
  exit 2
fi

if grep -RInE 'Arc<Mutex<CancellationToken>>|Mutex<CancellationToken>|max_turns' \
  agent/features/runtime/src agent/features/tools/src/adapters/agent_tool.rs agent/features/tools/src/domain/types/agent.rs \
  --include='*.rs' --exclude='*_tests.rs'; then # guard-registry:scope.runtime.shared-loop-tests
  echo '{"decision":"block","reason":"禁止恢复 Session token 槽或 max_turns；使用 per-Run scope + timeout + StuckGuard。"}'
  exit 2
fi

echo "Shared Run Loop guard OK."
