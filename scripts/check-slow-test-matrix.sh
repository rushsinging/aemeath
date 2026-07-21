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
cli_binary="${AEMEATH_PTY_BIN:-}"
if [[ -z "$cli_binary" ]]; then
  cli_binary="$(cargo build -p cli --bin aemeath --locked --message-format=json 2>/dev/null \
    | python3 -c 'import json, sys; paths = [item["executable"] for item in map(json.loads, sys.stdin) if item.get("reason") == "compiler-artifact" and item.get("target", {}).get("name") == "aemeath" and item.get("executable")]; print(paths[-1] if paths else "")')"
fi
if [[ -z "$cli_binary" || ! -f "$cli_binary" ]]; then
  echo "PTY binary missing after cli build: ${cli_binary:-<none>}" >&2
  exit 1
fi
run pty env AEMEATH_PTY_BIN="$cli_binary" cargo test -p cli --test pty_smoke -- --ignored --nocapture

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
