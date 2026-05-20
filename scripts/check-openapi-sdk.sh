#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
OPENAPI_JSON="$ROOT/packages/sdk/openapi/aemeath.json"

cd "$ROOT"

cargo run -p server --bin export_openapi -- "$OPENAPI_JSON"
(
  cd "$ROOT/packages/sdk/ts"
  pnpm install --frozen-lockfile
  pnpm generate:schema
  pnpm exec tsc --noEmit
)

git diff --exit-code -- packages/sdk/openapi/aemeath.json packages/sdk/ts/src/generated/schema.d.ts
