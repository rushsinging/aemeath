#!/usr/bin/env bash
set -euo pipefail

export AEMEATH_LOG_LEVEL="${AEMEATH_LOG_LEVEL:-debug}"

exec cargo run --bin aemeath -- "$@"
