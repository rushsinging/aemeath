#!/usr/bin/env bash
# Owner-aware production log target guard. Rust tests contain the maintained parser.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"

cd "$ROOT"
# Test paths, *_test.rs and *_tests.rs are deliberately excluded by the Rust
# scanner; this scope exclusion is registered in architecture-guard-registry.
# guard-registry:scope.logging.production-test-sources
cargo test -p logging domain::routing_guard::tests --quiet

echo "✓ owner-aware log target check passed"
