#!/bin/bash
set -euo pipefail

# #700：Main/Sub 必须只通过唯一 shared Loop Engine 驱动 Run。
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

ENGINE="agent/features/runtime/src/application/loop_engine/engine.rs"
MAIN="agent/features/runtime/src/application/loop_engine/chat/loop_runner.rs"
SUB="agent/features/runtime/src/application/run/derived/loop_run.rs"
LAUNCHER="agent/features/runtime/src/application/run/launcher.rs"
MAIN_PORT="agent/features/runtime/src/application/loop_engine/chat/main_run_port.rs"
STEP_PERSISTENCE="agent/features/runtime/src/application/loop_engine/step_persistence.rs"
INTERACTION_COORDINATOR="agent/features/runtime/src/application/interaction/coordinator.rs"
STOP_HOOK_COORDINATOR="agent/features/runtime/src/application/hook/stop_coordination.rs"
CONTEXT_COORDINATION="agent/features/runtime/src/application/context/coordination.rs"
OLD_FSM="agent/features/runtime/src/application/loop_engine/chat/state.rs"

for path in "$ENGINE" "$MAIN" "$MAIN_PORT" "$SUB" "$LAUNCHER" "$STEP_PERSISTENCE" "$INTERACTION_COORDINATOR" "$STOP_HOOK_COORDINATOR" "$CONTEXT_COORDINATION"; do
  if [ ! -f "$path" ]; then
    echo "{\"decision\":\"block\",\"reason\":\"共享 Loop Engine 守卫缺少文件：$path\"}"
    exit 2
  fi
done

if grep -RInF '#![allow(dead_code)]' \
    agent/features/runtime/src/application --include='*.rs'; then
  echo '{"decision":"block","reason":"Runtime application 禁止模块级 dead_code 豁免；必须分类接线、cfg(test) 或物理删除。"}'
  exit 2
fi

if [ -e "$OLD_FSM" ]; then
  echo "{\"decision\":\"block\",\"reason\":\"旧 ChatLoopState FSM 禁止恢复：$OLD_FSM\"}"
  exit 2
fi

engine_defs=$(grep -RInE 'pub([[:space:]]*\([^)]*\))?[[:space:]]+async[[:space:]]+fn[[:space:]]+run_loop([[:space:]]*<[^>]+>)?[[:space:]]*\(' \
  agent/features/runtime/src/application --include='*.rs' --exclude='*_tests.rs' | wc -l | tr -d ' ') # guard-registry:scope.runtime.shared-loop-tests
if [ "$engine_defs" -ne 1 ]; then
  echo "{\"decision\":\"block\",\"reason\":\"生产代码必须恰有一个共享 run_loop 定义，当前数量：$engine_defs\"}"
  exit 2
fi

# #1397: Main/Derived must call the single RunInstance launcher; the launcher
# owns the execute_prepared_loop entry and all legacy launch bridges are retired.
if ! grep -q 'execute_prepared_loop(' "$LAUNCHER"; then
  echo '{"decision":"block","reason":"RunLauncher 未调用统一 execute_prepared_loop 入口。"}'
  exit 2
fi
if ! grep -q 'pub async fn launch(' "$LAUNCHER" ||
   ! grep -q 'instance: &mut RunInstance' "$LAUNCHER"; then
  echo '{"decision":"block","reason":"RunLauncher 必须以 launch 动词入口消费完整 RunInstance。"}'
  exit 2
fi
if grep -q 'pub async fn launch<' "$LAUNCHER" ||
   grep -q 'pub async fn launch_prepared' "$LAUNCHER"; then
  echo '{"decision":"block","reason":"RunLauncher 禁止恢复旧 launch 泛型入口或 launch_prepared 兼容入口。"}'
  exit 2
fi

# Main/Derived both enter through launch and pass a complete RunInstance.
if ! grep -q 'run::launcher::launch(' "$MAIN" ||
   ! grep -q '&mut run_instance' "$MAIN"; then
  echo '{"decision":"block","reason":"Main Run 未将完整 RunInstance 交给统一 launch 入口。"}'
  exit 2
fi

if ! grep -q 'run::launcher::launch(' "$SUB" ||
   ! grep -q 'instance' "$SUB"; then
  echo '{"decision":"block","reason":"Derived Run 未将完整 RunInstance 交给统一 launch 入口。"}'
  exit 2
fi

if grep -q 'run_instance\.into_parts()' "$MAIN" ||
   grep -q 'run_instance\.into_parts()' "$SUB"; then
  echo '{"decision":"block","reason":"Run 来源禁止在启动前拆散 RunInstance。"}'
  exit 2
fi

if grep -RInE 'context::session::|\bChatChain\b|\bChatSegment\b|save_chain|current_chain|frozen_chats|active_summary|SessionProjectionParticipant|projection_start_index|microcompact_(chain|messages)|compact_messages_with_llm' \
    agent/features/runtime/src --include='*.rs' --exclude='*_tests.rs' --exclude-dir='tests'; then # guard-registry:scope.runtime.shared-loop-tests
  echo '{"decision":"block","reason":"Runtime 生产代码必须只经 Context crate-root Published Language / ContextPort 使用 Session，禁止内部类型、第二投影 backing、save callback 与 legacy compact helper。"}'
  exit 2
fi

if ! grep -q 'append_finalized' "$STEP_PERSISTENCE"; then
  echo '{"decision":"block","reason":"无角色 Step persistence owner 必须接入唯一 finalized Step append。"}'
  exit 2
fi
if grep -q 'append_finalized' "$MAIN_PORT" || grep -q 'append_finalized' "$SUB"; then
  echo '{"decision":"block","reason":"Main/Sub adapter 禁止各自保留 finalized Step append 算法。"}'
  exit 2
fi

if ! grep -q 'struct InteractionCompletionContext' "$INTERACTION_COORDINATOR" ||
   ! grep -q 'complete_tool_interaction' "$INTERACTION_COORDINATOR"; then
  echo '{"decision":"block","reason":"Interaction completion 必须由无角色 InteractionCoordinator 统一拥有。"}'
  exit 2
fi
if grep -q 'trait InteractionCompletionPort' "$ENGINE" ||
   grep -Eiq 'fn interaction_(execution_scope|tool_execution|materializer|session_id|cancellation)\(' "$MAIN_PORT" "$SUB"; then
  echo '{"decision":"block","reason":"Main/Sub adapter 禁止恢复 Interaction completion fat port 或角色 completion 方法。"}'
  exit 2
fi

if ! grep -q 'struct StopHookExecutionContext' "$STOP_HOOK_COORDINATOR" ||
   ! grep -q 'trait StopHookObserver' "$STOP_HOOK_COORDINATOR" ||
   ! grep -q 'coordinate_stop_hook' "$STOP_HOOK_COORDINATOR"; then
  echo '{"decision":"block","reason":"Stop Hook 必须由无角色 stop coordinator 统一执行并应用结果。"}'
  exit 2
fi
if grep -q 'trait StopHookPort' "$ENGINE" ||
   grep -Eiq 'fn (stop_hook_context|project_stop_hook_outcome)\(' "$MAIN_PORT" "$SUB"; then
  echo '{"decision":"block","reason":"Main/Sub adapter 禁止恢复 Stop Hook fat port 或角色化执行方法。"}'
  exit 2
fi

if grep -RInE 'Arc<Mutex<CancellationToken>>|Mutex<CancellationToken>|max_turns' \
  agent/features/runtime/src agent/features/tools/src/adapters/agent_tool.rs agent/features/tools/src/domain/types/agent.rs \
  --include='*.rs' --exclude='*_tests.rs'; then # guard-registry:scope.runtime.shared-loop-tests
  echo '{"decision":"block","reason":"禁止恢复 Session token 槽或 max_turns；使用 per-Run scope + timeout + StuckGuard。"}'
  exit 2
fi

echo "Shared Run Loop guard OK."
