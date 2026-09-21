#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cp -R "$ROOT/." "$TMP/repo"
GUARD="$TMP/repo/.agents/hooks/check-cost-tracker-retirement.sh"

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
    echo "[cost-tracker-retirement] $label did not fail as expected" >&2
    echo "$output" >&2
    exit 1
  fi
}

run_guard >/dev/null

mkdir -p "$TMP/repo/agent/features/runtime/src/application/cost"
printf '%s\n' 'pub struct RetiredPricingOwner;' >"$TMP/repo/agent/features/runtime/src/application/cost/pricing.rs"
expect_failure runtime-cost-path "retired Runtime Cost owner must stay absent"
rm -rf "$TMP/repo/agent/features/runtime/src/application/cost"

printf '%s\n' 'pub fn calculate_price() {}' >"$TMP/repo/agent/features/runtime/src/application/pricing.rs"
expect_failure runtime-pricing-path "retired Runtime Pricing implementation must stay absent"
rm -f "$TMP/repo/agent/features/runtime/src/application/pricing.rs"

printf '%s\n' 'pub fn calculate_cost() {}' >"$TMP/repo/agent/features/runtime/src/application/model_cost.rs"
expect_failure runtime-cost-file "retired Runtime Cost implementation must stay absent"
rm -f "$TMP/repo/agent/features/runtime/src/application/model_cost.rs"

printf '%s\n' 'pub struct CostTracker;' >"$TMP/repo/agent/features/runtime/src/application/retired_cost_probe.rs"
expect_failure cost-tracker-symbol "retired Cost surface must stay absent"
rm -f "$TMP/repo/agent/features/runtime/src/application/retired_cost_probe.rs"

printf '%s\n' 'pub const COST_HISTORY_FILE: &str = "cost_history.json";' >"$TMP/repo/agent/shared/src/config/adapters/retired_cost_path_probe.rs"
expect_failure cost-history-path "retired Cost surface must stay absent"
rm -f "$TMP/repo/agent/shared/src/config/adapters/retired_cost_path_probe.rs"

printf '%s\n' 'fn read_legacy_history() { let _ = std::fs::read("cost_history.json"); }' >"$TMP/repo/agent/features/storage/src/retired_cost_history_probe.rs"
expect_failure cost-history-literal "retired Cost history access must stay absent"
rm -f "$TMP/repo/agent/features/storage/src/retired_cost_history_probe.rs"

printf '%s\n' 'pub struct CostInfo { pub cost_usd: f64 }' >"$TMP/repo/packages/sdk/src/retired_cost_event_probe.rs"
expect_failure sdk-cost-surface "retired Cost surface must stay absent"
rm -f "$TMP/repo/packages/sdk/src/retired_cost_event_probe.rs"

printf '%s\n' 'fn legacy_namespace() { let _ = StorageNamespace::Cost; }' >"$TMP/repo/agent/features/storage/src/retired_cost_namespace_probe.rs"
expect_failure storage-cost-namespace "retired Cost surface must stay absent"
rm -f "$TMP/repo/agent/features/storage/src/retired_cost_namespace_probe.rs"

run_guard >/dev/null
echo "Cost retirement guard terminal boundary probes passed."
