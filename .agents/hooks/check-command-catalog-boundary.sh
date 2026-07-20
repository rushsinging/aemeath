#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_GUARD_ROOT:-$(git rev-parse --show-toplevel)}"
cd "$ROOT"

fail() {
  echo "[command-boundary] $1" >&2
  exit 2
}

# Tools owns the only Command Published Language and ports.
command_defs="$(grep -RInE '^[[:space:]]*pub[[:space:]]+(struct|enum|trait)[[:space:]]+(CommandDescriptor|CommandRoute|CommandCatalogPort|CommandRouterPort|SlashInput|CommandParseError)\b' \
  agent packages apps --include='*.rs' --exclude='*test*.rs' --exclude-dir=tests || true)" # guard-registry:scope.command.tests-and-owner-filter
if [ -n "$command_defs" ]; then
  invalid="$(printf '%s\n' "$command_defs" | grep -vE '^agent/features/tools/src/domain/command_(pl|ports)\.rs:' || true)" # guard-registry:scope.command.tests-and-owner-filter
  [ -z "$invalid" ] || { printf '%s\n' "$invalid" >&2; fail "Command PL/ports must be defined only by Tools"; }
fi

# Delivery must not restore a static builtin registry or independent command parser.
if grep -RInE '\bbuiltin_commands[[:space:]]*\(|SLASH_HELP_LINES|fn[[:space:]]+(is_exit_command|parse_reflection_history_command)[[:space:]]*\(' \
  packages/sdk/src apps/cli/src --include='*.rs' --exclude='*test*.rs'; then # guard-registry:scope.command.tests-and-owner-filter
  fail "delivery must consume CommandCatalogPort/CommandRouterPort instead of duplicate command truth"
fi

# Runtime may consume typed routes, but must not define a second command catalog/router.
if grep -RInE 'struct[[:space:]]+CommandDescriptor|trait[[:space:]]+Command(Catalog|Router)Port|enum[[:space:]]+CommandRoute' \
  agent/features/runtime/src --include='*.rs' --exclude='*test*.rs'; then # guard-registry:scope.command.tests-and-owner-filter
  fail "Runtime must not own Command Catalog/Router Published Language"
fi

echo "Command Catalog/Router boundary guard OK."
