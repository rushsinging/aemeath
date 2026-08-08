#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}}"
RUNTIME="$ROOT/agent/features/runtime/src"
INVOCATION="$RUNTIME/application/model/invocation.rs"
CONTEXT_REQUEST="$RUNTIME/application/loop_engine/context_request.rs"
RUN_SERVICES="$RUNTIME/application/loop_engine/run_services.rs"
MAIN_LAUNCH="$RUNTIME/application/loop_engine/chat/session_driver/run_launch.rs"
DERIVED_SETUP="$RUNTIME/application/run/derived/setup.rs"

python3 - "$INVOCATION" "$CONTEXT_REQUEST" "$RUN_SERVICES" "$MAIN_LAUNCH" "$DERIVED_SETUP" <<'PY'
import re
import sys
from pathlib import Path

invocation, context_request, run_services, main_launch, derived_setup = map(Path, sys.argv[1:])
violations = []

for path in (invocation, context_request, run_services, main_launch, derived_setup):
    if not path.is_file():
        violations.append(f"{path}: required production source is missing")

if violations:
    print("Provider window single-owner guard FAILED:\n" + "\n".join(violations), file=sys.stderr)
    raise SystemExit(2)

def production_text(path: Path) -> str:
    text = path.read_text()
    marker = re.search(r"(?m)^\s*#\[cfg\(test\)\]\s*$", text)
    return text[:marker.start()] if marker else text

invocation_source = production_text(invocation)
mapper_call = invocation_source.find("extract_invocation_context(&window)")
if mapper_call < 0:
    violations.append(f"{invocation}: missing ContextWindow mechanical mapper call")
else:
    tail = invocation_source[mapper_call:]
    if re.search(r"messages_for_api\s*\.\s*(push|insert|extend|remove|retain|append)\s*\(", tail):
        violations.append(f"{invocation}: Runtime must not decorate messages_for_api after ContextWindow mapping")

runtime_root = invocation.parents[2]
runtime_sources = list(runtime_root.rglob("*.rs"))
for path in runtime_sources:
    relative = path.relative_to(runtime_root)
    if "tests" in relative.parts or path.name.endswith("_tests.rs"):
        continue
    source = production_text(path)
    if "InvocationReminder" in source and re.search(r"<(?:system|task)-reminder>", source):
        violations.append(f"{path}: Runtime reminder intent production must not render Provider-visible tags")

context_request_source = production_text(context_request)
run_services_source = production_text(run_services)
if "invocation_reminders: vec![]" not in context_request_source:
    violations.append(f"{context_request}: ContextRequest coordinator must initialize typed reminder input")
if "request.invocation_reminders = self.context_request.invocation_reminders.clone()" not in run_services_source:
    violations.append(f"{run_services}: Runtime must map typed reminder intents into ContextRequest before build_window")

if "ContextRequestCoordinator::new(self.source()).build_request" not in run_services_source:
    violations.append(f"{run_services}: shared ContextRequest coordinator must remain the window input owner")

for path in (main_launch, derived_setup):
    source = production_text(path)
    if "RuntimeStepPersistence::new" not in source or "ContextRequestData" not in source:
        violations.append(f"{path}: Main/Sub must enter the shared ContextRequest pipeline")

if violations:
    print("Provider window single-owner guard FAILED:\n" + "\n".join(violations), file=sys.stderr)
    raise SystemExit(2)

print("Provider window single-owner guard OK.")
PY
