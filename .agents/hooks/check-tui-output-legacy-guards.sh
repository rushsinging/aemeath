#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

fail=0

if grep -R "find_last_running\|last running\|最后一个 running" "$ROOT/apps/cli/src/tui" -n --include='*.rs'; then
  echo "[architecture] output legacy fallback is forbidden after TUI M2" >&2
  fail=1
fi

# guard-registry:false-positive.tui.running-indicator-condition
if grep -R "cell\.set_char('●')" "$ROOT/apps/cli/src/tui/output_area" "$ROOT/apps/cli/src/tui/render" -n --include='*.rs' | grep -v "if matches!(line.style, LineStyle::ToolCallRunning)"; then
  echo "[architecture] render must not overwrite completed tool status icons" >&2
  fail=1
fi

TUI_CONVERSATION="$ROOT/apps/cli/src/tui/model/conversation"
TUI_RUNTIME_MAPPER="$ROOT/apps/cli/src/tui/adapter/agent_event.rs"

if grep -R -nE 'AgentRun(Phase|State|StepPhase|StepState)|AgentRunChanged|AgentRunStepChanged|agent_runs|terminal_agent_runs' \
  "$TUI_CONVERSATION" --include='*.rs'; then
  echo "[architecture] ConversationModel must not retain a parallel Run lifecycle state source" >&2
  fail=1
fi

if grep -nE 'ConversationIntent::Run(Started|AwaitingUser|Resumed|Cancelling|Cancelled|Completed|Failed|StepStarted|StepCompleted)' \
  "$TUI_RUNTIME_MAPPER"; then
  echo "[architecture] Runtime Run lifecycle observations must not mutate ConversationModel" >&2
  fail=1
fi

exit "$fail"
