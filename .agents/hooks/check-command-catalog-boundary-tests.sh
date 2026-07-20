#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
GUARD="$ROOT/.agents/hooks/check-command-catalog-boundary.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

cp -R "$ROOT/agent" "$ROOT/apps" "$ROOT/packages" "$TMP/"

run_guard() {
  AEMEATH_GUARD_ROOT="$TMP" bash "$GUARD" >/dev/null 2>&1
}

run_guard

mkdir -p "$TMP/packages/sdk/src"
printf '\npub fn builtin_commands() {}\n' >> "$TMP/packages/sdk/src/commands.rs"
if run_guard; then
  echo "expected duplicate SDK builtin command registry to fail" >&2
  exit 1
fi
cp "$ROOT/packages/sdk/src/commands.rs" "$TMP/packages/sdk/src/commands.rs"

printf '\nfn parse_reflection_history_command(_: &str) {}\n' >> "$TMP/apps/cli/src/chat/no_tui.rs"
if run_guard; then
  echo "expected delivery-local slash parser to fail" >&2
  exit 1
fi
cp "$ROOT/apps/cli/src/chat/no_tui.rs" "$TMP/apps/cli/src/chat/no_tui.rs"

printf '\npub enum CommandRoute {}\n' >> "$TMP/agent/features/runtime/src/lib.rs"
if run_guard; then
  echo "expected Runtime duplicate CommandRoute to fail" >&2
  exit 1
fi
cp "$ROOT/agent/features/runtime/src/lib.rs" "$TMP/agent/features/runtime/src/lib.rs"

run_guard
echo "Command Catalog/Router boundary negative probes passed."
