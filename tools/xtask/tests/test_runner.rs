//! `xtask test-runner` CLI 级测试（PATH stub 手法）：
//! fake cargo 注入 PATH，验证逐包编排、超时收割、exit code 传播与环境净化。
//!
//! 移植自 `.agents/hooks/check-unit-tests-tests.sh` 的四组断言：
//! 超时 exit 124 + 进程组收割 + fail-fast；首包失败 exit 原样传播；
//! git repository-local 环境变量不得到达 cargo；composition 恰好跑 `--tests` 一次。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const GUARD_BIN: &str = env!("CARGO_BIN_EXE_xtask");

fn write_executable(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("parent dir")).expect("create parent");
    fs::write(path, content).expect("write stub");
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod");
}

const FAKE_CARGO: &str = r#"#!/usr/bin/env bash
set -euo pipefail

package=""
args=("$@")
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-p" ]; then
    package="$2"
    break
  fi
  shift
done

printf '%s\n' "$package" >>"$FAKE_CARGO_LOG"
printf '%s\t' "$package" >>"$FAKE_CARGO_ARGS_LOG"
printf '%s ' "${args[@]}" >>"$FAKE_CARGO_ARGS_LOG"
printf '\n' >>"$FAKE_CARGO_ARGS_LOG"
printf 'GIT_DIR=%s\n' "${GIT_DIR-<unset>}" >>"$FAKE_CARGO_ENV_LOG"
printf 'GIT_WORK_TREE=%s\n' "${GIT_WORK_TREE-<unset>}" >>"$FAKE_CARGO_ENV_LOG"

case "${FAKE_CARGO_MODE}:${package}" in
  timeout:share)
    printf '%s\n' "$$" >"$FAKE_CARGO_PID_FILE"
    sleep 5
    ;;
  fail:share)
    exit 7
    ;;
esac
echo "test result: ok. 1 passed; 0 failed"
"#;

struct StubFixture {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    bin_dir: PathBuf,
    cargo_log: PathBuf,
    pid_file: PathBuf,
}

fn make_fixture() -> StubFixture {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let bin_dir = temp.path().join("bin");
    // test-runner 需枚举 git local env 变量：fixture 必须是 git 仓库。
    fs::create_dir_all(&repo).expect("create repo");
    let init = Command::new("git")
        .args(["init", "-q"])
        .arg(&repo)
        .output()
        .expect("git init");
    assert!(init.status.success());
    write_executable(&bin_dir.join("cargo"), FAKE_CARGO);
    StubFixture {
        repo,
        bin_dir,
        cargo_log: temp.path().join("cargo.log"),
        pid_file: temp.path().join("cargo.pid"),
        _temp: temp,
    }
}

fn run_test_runner(fixture: &StubFixture, mode: &str) -> std::process::Output {
    Command::new(GUARD_BIN)
        .arg("test-runner")
        .env("AEMEATH_PROJECT_DIR", &fixture.repo)
        .env("AEMEATH_UNIT_TEST_TIMEOUT_SECS", "1")
        .env("CARGO_TARGET_DIR", fixture.repo.join("target/hook-tests"))
        .env("FAKE_CARGO_MODE", mode)
        .env("FAKE_CARGO_LOG", &fixture.cargo_log)
        .env("FAKE_CARGO_ARGS_LOG", fixture.repo.join("cargo-args.log"))
        .env("FAKE_CARGO_ENV_LOG", fixture.repo.join("cargo-env.log"))
        .env("FAKE_CARGO_PID_FILE", &fixture.pid_file)
        // 模拟 git hook 注入的仓库本地变量，断言它们不得到达 cargo。
        .env("GIT_DIR", fixture.repo.join("caller.git"))
        .env("GIT_WORK_TREE", fixture.repo.join("caller-worktree"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                fixture.bin_dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .expect("spawn xtask test-runner")
}

#[test]
fn test_runner_timeout_reaps_process_group_and_fails_fast() {
    let fixture = make_fixture();

    let output = run_test_runner(&fixture, "timeout");

    assert_eq!(
        output.status.code(),
        Some(124),
        "超时包必须 exit 124，stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("share") && stderr.contains("1s"),
        "超时输出必须指明包与限时: {stderr}"
    );
    let timed_out_pid: i32 = fs::read_to_string(&fixture.pid_file)
        .expect("read pid")
        .trim()
        .parse()
        .expect("parse pid");
    let alive = Command::new("kill")
        .args(["-0", &timed_out_pid.to_string()])
        .status()
        .expect("kill -0")
        .success();
    assert!(!alive, "超时 cargo 进程必须被收割: pid={timed_out_pid}");
    let cargo_log = fs::read_to_string(&fixture.cargo_log).expect("read cargo log");
    assert_eq!(
        cargo_log.lines().count(),
        1,
        "超时后必须 fail-fast（不跑下一包）: {cargo_log}"
    );
}

#[test]
fn test_runner_propagates_first_failure_exit_code() {
    let fixture = make_fixture();

    let output = run_test_runner(&fixture, "fail");

    assert_eq!(
        output.status.code(),
        Some(7),
        "首包失败必须原样传播 exit 7，stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cargo_log = fs::read_to_string(&fixture.cargo_log).expect("read cargo log");
    assert_eq!(
        cargo_log.trim(),
        "share",
        "fail-fast 必须停在 share: {cargo_log}"
    );
}

#[test]
fn test_runner_strips_git_local_env_before_cargo() {
    let fixture = make_fixture();

    let output = run_test_runner(&fixture, "pass");

    assert!(
        output.status.success(),
        "pass 模式必须 exit 0，stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let env_log = fs::read_to_string(fixture.repo.join("cargo-env.log")).expect("read env log");
    assert!(
        env_log.lines().all(|line| line.ends_with("=<unset>")),
        "git 仓库本地环境不得到达 cargo: {env_log}"
    );
}

#[test]
fn test_runner_package_matrix_and_special_targets() {
    let fixture = make_fixture();

    let output = run_test_runner(&fixture, "pass");

    assert!(output.status.success());
    let cargo_log = fs::read_to_string(&fixture.cargo_log).expect("read cargo log");
    let composition_runs = cargo_log
        .lines()
        .filter(|line| *line == "composition")
        .count();
    assert_eq!(
        composition_runs, 1,
        "composition 必须恰好跑一次: {cargo_log}"
    );
    let args_log = fs::read_to_string(fixture.repo.join("cargo-args.log")).expect("read args");
    let composition_line = args_log
        .lines()
        .find(|line| line.starts_with("composition\t"))
        .expect("composition args");
    assert!(
        composition_line.contains("test -p composition --tests"),
        "composition 必须跑全部集成测试: {composition_line}"
    );
    let cli_line = args_log
        .lines()
        .find(|line| line.starts_with("cli\t"))
        .expect("cli args");
    assert!(
        cli_line.contains("test -p cli --bin aemeath"),
        "cli 必须只跑 bin target: {cli_line}"
    );
}
