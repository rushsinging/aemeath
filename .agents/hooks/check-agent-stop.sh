#!/usr/bin/env bash
# Agent Stop gate: keep feedback immediate without repeating Cargo-backed checks.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/check-architecture-guards.sh" --fast
