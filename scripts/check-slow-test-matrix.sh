#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export CI=1
export INSTA_UPDATE=no

run() { local label=$1; shift; local started=$SECONDS; echo "==> $label"; "$@"; echo "<== $label $((SECONDS-started))s"; }

run fmt cargo fmt --all -- --check
run clippy cargo clippy --workspace --all-targets -- -D warnings
run workspace cargo test --workspace --locked --exclude cli
run tui-p0 scripts/check-tui-snapshots.sh
run tui-p1 cargo test -p cli 'scenario_tests::p1'
run cli-build cargo build -p cli --bin aemeath --locked
run pty env AEMEATH_PTY_BIN="$ROOT/target/debug/aemeath" cargo test -p cli --test pty_smoke -- --ignored --nocapture

targets=()
if [[ "${AEMEATH_MATRIX_CROSS:-0}" == "1" ]]; then
  case "$(uname -s)" in
    Darwin) targets=(aarch64-apple-darwin x86_64-apple-darwin) ;;
    Linux) targets=(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu) ;;
    *) targets=() ;;
  esac
else
  echo "SKIP cross-platform builds: set AEMEATH_MATRIX_CROSS=1 to enable"
fi

for target in ${targets[@]+"${targets[@]}"}; do
  if ! rustup target list --installed | grep -qx "$target"; then echo "SKIP platform-build $target: rustup target 未安装"; continue; fi
  if [[ "$target" == aarch64-unknown-linux-gnu ]] && ! command -v aarch64-linux-gnu-gcc >/dev/null; then echo "SKIP platform-build $target: linker 未安装"; continue; fi
  if [[ "$target" == x86_64-unknown-linux-gnu ]] && ! command -v x86_64-linux-gnu-gcc >/dev/null && [[ "$(rustc -vV | awk '/host:/ {print $2}')" != "$target" ]]; then echo "SKIP platform-build $target: linker 未安装"; continue; fi
  run "platform-build $target" cargo build -p cli --bin aemeath --locked --target "$target"
done
