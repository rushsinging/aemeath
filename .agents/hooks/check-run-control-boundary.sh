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

if grep -nE 'cancel_run_step|terminate_run' "$CLIENT"; then
  echo "New run control APIs must not reach production AgentClient before #878 atomic cutover." >&2
  fail=1
fi

exit "$fail"
