#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cp -R "$ROOT/." "$TMP/repo"
GUARD="$TMP/repo/.agents/hooks/check-runtime-large-file-responsibilities.sh"

run_guard() {
  AEMEATH_PROJECT_DIR="$TMP/repo" /bin/bash "$GUARD"
}

expect_failure() {
  local label="$1"
  local expected="$2"
  local output
  local status=0
  output="$(run_guard 2>&1)" || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[runtime-large-file-responsibilities] $label did not fail as expected" >&2
    echo "$output" >&2
    exit 1
  fi
}

run_guard >/dev/null

printf '\n// probe\n' >>"$TMP/repo/agent/features/runtime/src/application/loop_engine/engine.rs"
python3 - "$TMP/repo/agent/features/runtime/src/application/loop_engine/engine.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
path.write_text(source + "\n".join("// oversized root probe" for _ in range(401)) + "\n")
PY
expect_failure oversized-engine "responsibility budget"

rm -f "$TMP/repo/agent/features/runtime/src/application/loop_engine/engine/contracts.rs"
expect_failure missing-owner "required #1400 responsibility owner is missing"

mkdir -p "$TMP/repo/agent/features/runtime/src/application/loop_engine/engine/probe"
touch "$TMP/repo/agent/features/runtime/src/application/loop_engine/engine/probe/mod.rs"
expect_failure mod-rs "must not introduce mod.rs"

printf '%s\n' 'fn probe(_: ChatLoopContext<(), ()>) {}' >>"$TMP/repo/agent/features/runtime/src/application/client/accessors.rs"
expect_failure retired-symbol "restored a retired compatibility symbol"

echo "Runtime Large File Responsibilities terminal boundary probes passed."
