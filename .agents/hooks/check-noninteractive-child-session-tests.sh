#!/bin/bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
GUARD="$ROOT/.agents/hooks/check-noninteractive-child-session.sh"

if [ ! -x "$GUARD" ]; then
  echo "guard script missing: $GUARD" >&2
  exit 1
fi

fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT
mkdir -p "$fixture_root/apps/cli/src" "$fixture_root/packages/global/utils/src"

cat > "$fixture_root/apps/cli/src/bad.rs" <<'RS'
fn run() {
    let mut command = std::process::Command::new("git");
    let _ = command.output();
}
RS
if AEMEATH_PROJECT_DIR="$fixture_root" "$GUARD" >/dev/null 2>&1; then
  echo "guard must reject an unconfigured production command" >&2
  exit 1
fi

cat > "$fixture_root/apps/cli/src/bad.rs" <<'RS'
fn run() {
    let mut command = std::process::Command::new("git");
    utils::configure_std_noninteractive(&mut command)?;
    let _ = command.output();
}
RS
mkdir -p "$fixture_root/apps/cli/src/tests"
cat > "$fixture_root/apps/cli/src/tests/ignored.rs" <<'RS'
fn test_only() { let _ = std::process::Command::new("git").output(); }
RS
cat > "$fixture_root/packages/global/utils/src/process.rs" <<'RS'
pub fn configure() { unsafe { libc::setsid(); } }
RS
AEMEATH_PROJECT_DIR="$fixture_root" "$GUARD"

echo "[check-noninteractive-child-session-tests] positive and negative fixtures passed."
