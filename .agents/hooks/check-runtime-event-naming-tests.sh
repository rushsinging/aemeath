#!/bin/bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
GUARD="$ROOT/.agents/hooks/check-runtime-event-naming.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

cp -R "$ROOT/agent" "$ROOT/apps" "$ROOT/packages" "$ROOT/docs" "$TMP/"
mkdir -p "$TMP/.agents"
cp "$ROOT/.agents/runtime-event-naming-baseline.json" "$TMP/.agents/"

run_guard() {
  AEMEATH_GUARD_ROOT="$TMP" /bin/bash "$GUARD" >/dev/null 2>&1
}

expect_failure() {
  local description="$1"
  if run_guard; then
    echo "expected event naming guard to reject: $description" >&2
    exit 1
  fi
}

run_guard

python3 - "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text().replace("\n}\n\npub trait ChatEventSink", "\n    RuntimeDataUpdated,\n}\n\npub trait ChatEventSink", 1)
path.write_text(source)
PY
expect_failure "broad Updated/Data event name"
cp "$ROOT/agent/features/runtime/src/application/loop_engine/chat/events.rs" \
  "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs"

python3 - "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text().replace("\n}\n\npub trait ChatEventSink", "\n    UndocumentedRuntimeEvent,\n}\n\npub trait ChatEventSink", 1)
path.write_text(source)
PY
expect_failure "event missing from the event index"
cp "$ROOT/agent/features/runtime/src/application/loop_engine/chat/events.rs" \
  "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs"

python3 - "$TMP/apps/cli/src/tui/adapter/tui_runtime_event.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
source = source.replace("RuntimeStatusChanged {", "ContextUsageEvent {", 1)
path.write_text(source)
PY
expect_failure "cross-layer Current fact name drift"
cp "$ROOT/apps/cli/src/tui/adapter/tui_runtime_event.rs" \
  "$TMP/apps/cli/src/tui/adapter/tui_runtime_event.rs"

python3 - "$TMP/packages/sdk/src/chat_event.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text().replace("\n}\n\n/// `SessionResumeFailed`", "\n    CancelRunStepCancelled,\n}\n\n/// `SessionResumeFailed`", 1)
path.write_text(source)
PY
expect_failure "command ACK using lifecycle terminal suffix"
cp "$ROOT/packages/sdk/src/chat_event.rs" "$TMP/packages/sdk/src/chat_event.rs"

python3 - "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text().replace(
    "\n}\n\npub trait ChatEventSink",
    "\n    ToolCallUpdate { context: RuntimeRunContext },\n}\n\npub trait ChatEventSink",
    1,
)
path.write_text(source)
PY
expect_failure "SDK compatibility event restored as Runtime producer"
cp "$ROOT/agent/features/runtime/src/application/loop_engine/chat/events.rs" \
  "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs"

python3 - "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text().replace(
    "\n}\n\npub trait ChatEventSink",
    "\n    Text { context: RuntimeRunContext, text: String },\n    Thinking { context: RuntimeRunContext, text: String },\n}\n\npub trait ChatEventSink",
    1,
)
path.write_text(source)
PY
expect_failure "legacy SDK text compatibility events restored as Runtime producers"
cp "$ROOT/agent/features/runtime/src/application/loop_engine/chat/events.rs" \
  "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs"

python3 - "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text().replace(
    "\n}\n\npub trait ChatEventSink",
    "\n    ToolCallStart { context: RuntimeRunContext },\n}\n\npub trait ChatEventSink",
    1,
)
path.write_text(source)
PY
expect_failure "legacy SDK ToolCallStart restored as Runtime producer"
cp "$ROOT/agent/features/runtime/src/application/loop_engine/chat/events.rs" \
  "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs"

python3 - "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text().replace(
    "\n}\n\npub trait ChatEventSink",
    "\n    ToolProgress { context: RuntimeRunContext },\n}\n\npub trait ChatEventSink",
    1,
)
path.write_text(source)
PY
expect_failure "legacy SDK ToolProgress restored as Runtime producer"
cp "$ROOT/agent/features/runtime/src/application/loop_engine/chat/events.rs" \
  "$TMP/agent/features/runtime/src/application/loop_engine/chat/events.rs"

python3 - "$TMP/packages/sdk/src/chat_event.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text().replace(
    "\n}\n\n/// `SessionResumeFailed`",
    "\n    CompactProgress { stage: String, current: Option<u32>, total: Option<u32> },\n}\n\n/// `SessionResumeFailed`",
    1,
)
path.write_text(source)
PY
expect_failure "retired stringly CompactProgress"
cp "$ROOT/packages/sdk/src/chat_event.rs" "$TMP/packages/sdk/src/chat_event.rs"

run_guard
echo "Runtime event naming negative probes passed."
