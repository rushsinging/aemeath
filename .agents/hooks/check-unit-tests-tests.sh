#!/usr/bin/env bash
# Regression tests for check-unit-tests.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOOK="$SCRIPT_DIR/check-unit-tests.sh"
tmp=""

cleanup() {
  if [ -n "$tmp" ]; then
    rm -rf -- "$tmp"
  fi
}

trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

make_repo() {
  local repo="$1"
  mkdir -p "$repo/.agents/hooks"
}

make_fake_cargo() {
  local bin_dir="$1"
  mkdir -p "$bin_dir"
  cat >"$bin_dir/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

package=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-p" ]; then
    package="$2"
    break
  fi
  shift
done

printf '%s\n' "$package" >>"$FAKE_CARGO_LOG"

case "${FAKE_CARGO_MODE}:${package}" in
  timeout:share)
    printf '%s\n' "$$" >"$FAKE_CARGO_PID_FILE"
    sleep 2
    ;;
  fail:share)
    exit 7
    ;;
esac
EOF
  chmod +x "$bin_dir/cargo"
}

run_hook() {
  local repo="$1"
  local bin_dir="$2"
  local mode="$3"
  local log="$4"
  local pid_file="$5"
  AEMEATH_PROJECT_DIR="$repo" \
    AEMEATH_UNIT_TEST_TIMEOUT_SECS=1 \
    CARGO_TARGET_DIR="$repo/target/hook-tests" \
    FAKE_CARGO_MODE="$mode" \
    FAKE_CARGO_LOG="$log" \
    FAKE_CARGO_PID_FILE="$pid_file" \
    PATH="$bin_dir:$PATH" \
    "$HOOK" 2>&1
}

main() {
  local repo bin_dir log pid_file output status timed_out_pid
  tmp="$(mktemp -d)"

  repo="$tmp/repo"
  bin_dir="$tmp/bin"
  log="$tmp/cargo.log"
  pid_file="$tmp/cargo.pid"
  make_repo "$repo"
  make_fake_cargo "$bin_dir"

  set +e
  output="$(run_hook "$repo" "$bin_dir" timeout "$log" "$pid_file")"
  status=$?
  set -e
  [ "$status" -eq 124 ] \
    || fail "timed-out crate must exit 124, got $status; output=$output"
  printf '%s' "$output" | grep -q 'share.*1s' \
    || fail "timeout output must identify package and limit: $output"
  timed_out_pid="$(cat "$pid_file")"
  if kill -0 "$timed_out_pid" 2>/dev/null; then
    fail "timed-out cargo process must be reaped: pid=$timed_out_pid"
  fi
  [ "$(wc -l <"$log" | tr -d ' ')" -eq 1 ] \
    || fail "timeout must stop before the next crate: $(cat "$log")"

  : >"$log"
  set +e
  output="$(run_hook "$repo" "$bin_dir" fail "$log" "$pid_file")"
  status=$?
  set -e
  [ "$status" -eq 7 ] \
    || fail "first crate failure must propagate exit 7, got $status; output=$output"
  [ "$(wc -l <"$log" | tr -d ' ')" -eq 1 ] \
    || fail "failure must stop before the next crate: $(cat "$log")"
  [ "$(cat "$log")" = "share" ] \
    || fail "failure-fast should stop on share: $(cat "$log")"

  echo "check-unit-tests hook regression tests passed"
}

main "$@"
