#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GUARD="$ROOT/.agents/hooks/check-provider-window-single-owner.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p \
  "$TMP/agent/features/runtime/src/application/model" \
  "$TMP/agent/features/runtime/src/application/loop_engine/chat/session_driver" \
  "$TMP/agent/features/runtime/src/application/loop_engine" \
  "$TMP/agent/features/runtime/src/application/run/derived"

cat >"$TMP/agent/features/runtime/src/application/model/invocation.rs" <<'RS'
fn invoke(window: &ContextWindow) {
    let invocation_context = extract_invocation_context(&window);
    consume(invocation_context);
}
RS
cat >"$TMP/agent/features/runtime/src/application/loop_engine/context_request.rs" <<'RS'
fn build() {
    ContextRequest { invocation_reminders: vec![] };
}
RS
cat >"$TMP/agent/features/runtime/src/application/loop_engine/run_services.rs" <<'RS'
fn build() {
    let mut request = ContextRequestCoordinator::new(self.source()).build_request();
    request.invocation_reminders = self.context_request.invocation_reminders.clone();
}
RS
cat >"$TMP/agent/features/runtime/src/application/loop_engine/chat/session_driver/run_launch.rs" <<'RS'
fn main_run() { RuntimeStepPersistence::new(ContextRequestData {}); }
RS
cat >"$TMP/agent/features/runtime/src/application/run/derived/setup.rs" <<'RS'
fn sub_run() { RuntimeStepPersistence::new(ContextRequestData {}); }
RS

run_guard() { AEMEATH_PROJECT_DIR="$TMP" "$GUARD"; }
expect_failure() {
  local needle="$1"
  local output status=0
  output="$(run_guard 2>&1)" || status=$?
  if [ "$status" -ne 2 ] || [[ "$output" != *"$needle"* ]]; then
    echo "expected guard failure containing: $needle" >&2
    echo "$output" >&2
    exit 1
  fi
}

run_guard >/dev/null
python3 - "$TMP/agent/features/runtime/src/application/model/invocation.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text()
path.write_text(text.replace("consume(invocation_context);", "invocation_context.messages_for_api.push(message);"))
PY
expect_failure 'must not decorate messages_for_api'

cat >"$TMP/agent/features/runtime/src/application/model/invocation.rs" <<'RS'
fn invoke(window: &ContextWindow) {
    let invocation_context = extract_invocation_context(&window);
    consume(invocation_context);
}
fn render() { let intent: InvocationReminder = reminder(); let text = "<system-reminder>bad</system-reminder>"; }
RS
expect_failure 'must not render Provider-visible tags'

echo 'Provider window single-owner guard sanity checks passed.'
