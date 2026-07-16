#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export CI=1
export INSTA_UPDATE=no

cargo test -p cli scenario_tests

if find apps/cli -type f \( -name '*.snap.new' -o -name '.pending-snap' \) -print -quit | grep -q .; then
    echo 'error: pending insta snapshot files found' >&2
    exit 1
fi
