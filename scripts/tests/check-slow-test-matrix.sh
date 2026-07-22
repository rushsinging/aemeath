#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT/scripts/check-slow-test-matrix.sh"
source_text="$(<"$SCRIPT")"

if ! grep -Fq 'cargo build -p cli --bin aemeath --locked --message-format=json' <<<"$source_text"; then
    echo "slow matrix must derive the CLI executable from Cargo build output" >&2
    exit 1
fi

if ! grep -Fq 'AEMEATH_PTY_BIN="$cli_binary"' <<<"$source_text"; then
    echo "slow matrix must pass the resolved CLI executable to PTY smoke" >&2
    exit 1
fi

if grep -Fq 'AEMEATH_PTY_BIN="$ROOT/target/debug/aemeath"' <<<"$source_text"; then
    echo "slow matrix must not hard-code the repository target directory" >&2
    exit 1
fi
