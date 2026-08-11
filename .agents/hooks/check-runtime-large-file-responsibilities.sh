#!/bin/bash
# guard-registry:policy.runtime.large-file-responsibilities
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
RUNTIME_SRC="$ROOT/agent/features/runtime/src"

fail=0
report() {
  echo "$1" >&2
  fail=1
}

check_max_lines() {
  local relative_path="$1"
  local maximum="$2"
  local path="$ROOT/$relative_path"
  if [ ! -f "$path" ]; then
    report "$relative_path: required split owner is missing"
    return
  fi
  local lines
  lines=$(wc -l <"$path" | tr -d ' ')
  if [ "$lines" -gt "$maximum" ]; then
    report "$relative_path: $lines lines exceeds the #1400 responsibility budget $maximum"
  fi
}

# The post-migration roots stay orchestration façades. Details belong to named
# sibling modules so responsibilities cannot silently collapse back together.
check_max_lines "agent/features/runtime/src/application/loop_engine/engine.rs" 400
check_max_lines "agent/features/runtime/src/application/loop_engine/chat/session_driver.rs" 400
check_max_lines "agent/features/runtime/src/application/client/from_args.rs" 550

for required in \
  application/loop_engine/engine/contracts.rs \
  application/loop_engine/engine/phases.rs \
  application/loop_engine/engine/step_driver.rs \
  application/loop_engine/engine/interaction_driver.rs \
  application/loop_engine/engine/control_driver.rs \
  application/loop_engine/chat/session_driver/run_preparation.rs \
  application/loop_engine/chat/session_driver/run_launch.rs \
  application/client/from_args_tests.rs
do
  [ -f "$RUNTIME_SRC/$required" ] || report "agent/features/runtime/src/$required: required #1400 responsibility owner is missing"
done

if find "$RUNTIME_SRC" -name mod.rs -print -quit | grep -q .; then
  report "Runtime large-file split must not introduce mod.rs"
fi

if rg -n '\b(process_chat_loop|ChatLoopContext|RuntimeResources|ChatRuntimeContext|RunLoopPort|MainRunPort|SubAgentRun|RuntimeContextParts)\b' \
  "$RUNTIME_SRC/application" --glob '*.rs' >/tmp/aemeath-runtime-large-file-retired-all.$$; then
  while IFS=: read -r path line_number source_line; do
    case "$path" in
      *_tests.rs|*/tests.rs|*/tests/*) continue ;;
    esac
    printf '%s:%s:%s\n' "$path" "$line_number" "$source_line" >>/tmp/aemeath-runtime-large-file-retired.$$
  done </tmp/aemeath-runtime-large-file-retired-all.$$
fi
rm -f /tmp/aemeath-runtime-large-file-retired-all.$$
if [ -s /tmp/aemeath-runtime-large-file-retired.$$ ]; then
  cat /tmp/aemeath-runtime-large-file-retired.$$ >&2
  report "Runtime large-file split restored a retired compatibility symbol"
fi
rm -f /tmp/aemeath-runtime-large-file-retired.$$

if [ "$fail" -ne 0 ]; then
  echo "Runtime Large File Responsibilities guard FAILED." >&2
  exit 2
fi

echo "Runtime Large File Responsibilities guard OK."
