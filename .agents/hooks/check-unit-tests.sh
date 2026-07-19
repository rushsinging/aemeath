#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
# Guard: if AEMEATH_PROJECT_DIR does not point at a project root, fall back to
# BASH_SOURCE. This keeps direct script execution and stale hook env values safe.
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi
cd "$ROOT"

echo "[hook-env] AEMEATH_PROJECT_DIR=${AEMEATH_PROJECT_DIR:-<unset>}"
echo "[hook-env] CLAUDE_PROJECT_DIR=${CLAUDE_PROJECT_DIR:-<unset>}"
echo "[hook-env] ROOT=$ROOT"
echo "[hook-env] PWD=$PWD"

# Keep hook builds isolated per checkout. Reusing the default/shared target dir
# across worktrees can leave stale crate metadata and make downstream crates see
# old public APIs from local path dependencies.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/hook-tests}"

crate_timeout_secs="${AEMEATH_UNIT_TEST_TIMEOUT_SECS:-180}"
if [[ ! "$crate_timeout_secs" =~ ^[1-9][0-9]*$ ]]; then
  echo "AEMEATH_UNIT_TEST_TIMEOUT_SECS must be a positive integer, got: $crate_timeout_secs" >&2
  exit 2
fi

run_with_timeout() {
  local package="$1"
  shift

  perl -e '
    use strict;
    use warnings;
    use POSIX qw(setpgid);

    my ($timeout, $package, @command) = @ARGV;
    my $pid = fork();
    die "fork failed: $!\n" unless defined $pid;

    if ($pid == 0) {
      setpgid(0, 0) or die "setpgid failed: $!\n";
      exec { $command[0] } @command;
      die "exec failed: $!\n";
    }

    my $timed_out = 0;
    local $SIG{ALRM} = sub {
      $timed_out = 1;
      kill "TERM", -$pid;
      select undef, undef, undef, 0.2;
      kill "KILL", -$pid;
    };

    alarm $timeout;
    waitpid($pid, 0);
    my $status = $?;
    alarm 0;

    if ($timed_out) {
      print STDERR "[hook-timeout] package $package exceeded ${timeout}s\n";
      exit 124;
    }
    if ($status == -1) {
      print STDERR "[hook-error] failed to wait for package $package: $!\n";
      exit 1;
    }
    if ($status & 127) {
      exit 128 + ($status & 127);
    }
    exit $status >> 8;
  ' "$crate_timeout_secs" "$package" "$@"
}

packages=(
  share
  workflow
  runtime
  project
  policy
  context
  provider
  tools
  storage
  hook
  audit
  cli
)

for package in "${packages[@]}"; do
  log_dir="$CARGO_TARGET_DIR/hook-logs"
  mkdir -p "$log_dir"
  log="$log_dir/${package}.log"
  if [[ "$package" == "cli" ]]; then
    echo "==> cargo test -p cli --bin aemeath (timeout: ${crate_timeout_secs}s)"
    set +e
    run_with_timeout "$package" cargo test -p cli --bin aemeath >"$log" 2>&1
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then
      echo "[hook] $package FAILED (rc=$rc); 完整日志: $log"
      grep -E 'error\[|error:|FAILED|panicked|failures:|^test .* FAILED' "$log" | head -n 40 || true
      exit "$rc"
    fi
  else
    echo "==> cargo test -p ${package} --lib (timeout: ${crate_timeout_secs}s)"
    set +e
    run_with_timeout "$package" cargo test -p "$package" --lib >"$log" 2>&1
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then
      echo "[hook] $package FAILED (rc=$rc); 完整日志: $log"
      grep -E 'error\[|error:|FAILED|panicked|failures:|^test .* FAILED' "$log" | head -n 40 || true
      exit "$rc"
    fi
  fi
  # 成功时 stdout 只留汇总行，保证 12 crate 总输出远低于宿主 8192 字节上限（#1220）。
  grep -E '^test result:' "$log" | tail -n 1 || echo "[hook] $package: (no test result line)"
  echo
done
