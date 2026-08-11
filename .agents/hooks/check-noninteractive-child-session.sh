#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "$ROOT/apps" ] && [ ! -d "$ROOT/agent" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

fail=0
# guard-registry:policy.process.noninteractive-child-session
while IFS= read -r file; do
  case "$file" in
    */tests/*|*/tests.rs|*_tests.rs|*_test.rs|*/packages/global/utils/src/process.rs|*/agent/features/project/src/lib.rs) continue ;;
  esac

  has_external_command=0
  if grep -Eq '(std::process::Command::new|tokio::process::Command::new)' "$file"; then
    has_external_command=1
  elif grep -Eq 'use[[:space:]]+(std|tokio)::process(::\{[^}]*Command[^}]*\}|::Command|[[:space:]]*;)' "$file" \
    && grep -Eq '\bCommand::new' "$file"; then
    has_external_command=1
  fi
  if [ "$has_external_command" -eq 0 ]; then
    continue
  fi
  if ! grep -Eq 'utils::configure_(std|tokio)_noninteractive' "$file"; then
    echo "$file: production external process construction must use utils::configure_*_noninteractive" >&2
    fail=1
  fi
  if grep -Eq '\.(process_group|pre_exec)\(' "$file" || grep -Eq '\blibc::setsid\(' "$file"; then
    echo "$file: session setup is owned only by packages/global/utils/src/process.rs" >&2
    fail=1
  fi
done < <(find "$ROOT/apps" "$ROOT/agent" "$ROOT/packages" -name '*.rs' -type f 2>/dev/null | sort)

if [ "$fail" -ne 0 ]; then
  echo '[architecture] 非交互外部进程必须经唯一 session 隔离边界。' >&2
  exit 2
fi

echo '[check-noninteractive-child-session] all production external processes use detached sessions.'
