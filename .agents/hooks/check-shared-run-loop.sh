#!/usr/bin/env bash
set -euo pipefail

# #700：Main/Sub 必须只通过唯一 shared Loop Engine 驱动 Run。
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

ENGINE="agent/features/runtime/src/application/loop_engine/engine.rs"
MAIN="agent/features/runtime/src/application/chat/looping/loop_runner.rs"
SUB="agent/features/runtime/src/application/agent/runner/loop_run.rs"
OLD_FSM="agent/features/runtime/src/application/chat/looping/state.rs"

for path in "$ENGINE" "$MAIN" "$SUB"; do
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

if ! grep -q 'run_loop(&mut run, &cancel, &mut port)' "$MAIN"; then
  echo '{"decision":"block","reason":"Main Run 未调用共享 loop_engine::run_loop。"}'
  exit 2
fi

if ! grep -q 'shared_run_loop(&mut run, &cancel, &mut self).await' "$SUB"; then
  echo '{"decision":"block","reason":"Sub Run 未调用共享 loop_engine::run_loop。"}'
  exit 2
fi

if grep -RInE 'Arc<Mutex<CancellationToken>>|Mutex<CancellationToken>|max_turns' \
  agent/features/runtime/src agent/features/tools/src/adapters/agent_tool.rs agent/features/tools/src/domain/types/agent.rs \
  --include='*.rs' --exclude='*_tests.rs'; then # guard-registry:scope.runtime.shared-loop-tests
  echo '{"decision":"block","reason":"禁止恢复 Session token 槽或 max_turns；使用 per-Run scope + timeout + StuckGuard。"}'
  exit 2
fi

echo "Shared Run Loop guard OK."
