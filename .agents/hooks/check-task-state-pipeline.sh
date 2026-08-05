#!/bin/bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
fail=0

report_matches() {
  local message="$1"
  shift
  local output
  output="$($@ 2>/dev/null || true)"
  if [ -n "$output" ]; then
    printf '%s\n' "$output" >&2
    printf '[architecture] %s\n' "$message" >&2
    fail=1
  fi
}

report_matches \
  "Task Runtime must not infer committed mutations from tool names or result prose." \
  grep -RInE 'is_task_store_mutation|"Status: Completed"' \
    "$ROOT/agent/features/runtime/src" --include='*.rs'

report_matches \
  "Retired string-only Task SDK/runtime snapshot symbols must not return." \
  grep -RInE 'TasksSnapshot|TaskStatusView' \
    "$ROOT/packages/sdk/src" "$ROOT/agent/features/runtime/src" "$ROOT/apps/cli/src/tui" \
    --include='*.rs'

for adapter in task_create.rs task_update.rs task_block_by.rs task_stop.rs task_list_create.rs task_list_complete.rs; do
  path="$ROOT/agent/features/tools/src/adapters/$adapter"
  if ! grep -q 'CommittedTaskChange::from_command_result' "$path"; then
    printf '%s\n' "$path" >&2
    printf '[architecture] Task mutation adapters must preserve committed change metadata.\n' >&2
    fail=1
  fi
done

exit "$fail"
