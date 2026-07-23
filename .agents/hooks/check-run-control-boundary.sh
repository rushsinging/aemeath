#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
SDK_RUN="$ROOT/packages/sdk/src/run.rs"
CLIENT="$ROOT/packages/sdk/src/client.rs"

fail=0
if grep -nE 'CancellationToken|Sender<|Receiver<|Mutex<|RwLock<|Arc<' "$SDK_RUN"; then
  echo "SDK run control Published Language must contain only pure value DTOs." >&2
  fail=1
fi

for api in cancel_run_step terminate_run; do
  if ! grep -q "fn ${api}" "$CLIENT"; then
    echo "Target Main run control API missing after #1247 cutover: ${api}." >&2
    fail=1
  fi
done

if grep -nE 'CancellationToken|Sender<|Receiver<|Mutex<|RwLock<|Arc<' "$CLIENT"; then
  echo "AgentClient Main run control commands must expose only pure value SDK types." >&2
  fail=1
fi

exit "$fail"
