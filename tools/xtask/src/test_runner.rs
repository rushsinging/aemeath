//! `xtask test-runner`：pre-push 单元测试门禁（原 `check-unit-tests.sh` 的 Rust 承接）。
//!
//! 语义对照原脚本：
//! - 清理 git hook 注入的仓库本地环境变量（`git rev-parse --local-env-vars`），
//!   防止 real-git fixture 的测试子进程把显式 cwd 覆盖为调用方仓库；
//! - `CARGO_TARGET_DIR` 缺省 `target/hook-tests`（按 checkout 隔离构建缓存）；
//! - 逐包 fail-fast 跑 `cargo test`，每包独立超时（进程组 TERM → KILL）；
//! - 包日志写入 `<target>/hook-logs/<package>.log`，失败时打印错误摘要；
//! - exit code 原样传播：超时 124，测试失败透传 cargo 退出码。

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

/// 逐包测试矩阵（原 shell `packages` 数组的数据化）。
/// `extra_args`：`--lib` 默认；cli 只跑 bin target；composition 跑全部集成测试。
pub struct PackageGate {
    pub package: &'static str,
    pub extra_args: &'static [&'static str],
}

pub const PACKAGE_GATES: &[PackageGate] = &[
    PackageGate {
        package: "share",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "runtime",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "project",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "policy",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "context",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "provider",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "tools",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "storage",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "hook",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "audit",
        extra_args: &["--lib"],
    },
    PackageGate {
        package: "composition",
        extra_args: &["--tests"],
    },
    PackageGate {
        package: "cli",
        extra_args: &["--bin", "aemeath"],
    },
];

/// 门禁失败：携带需原样传播的进程退出码。
#[derive(Debug)]
pub struct GateFailure {
    pub exit_code: i32,
    pub message: String,
}

impl std::fmt::Display for GateFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GateFailure {}

/// 清空 git hook 注入的仓库本地环境变量（`git rev-parse --local-env-vars`）。
pub fn strip_git_local_env(repo_root: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["rev-parse", "--local-env-vars"])
        .current_dir(repo_root)
        .output()
        .context("枚举 git 仓库本地环境变量失败")?;
    if !output.status.success() {
        anyhow::bail!(
            "git rev-parse --local-env-vars 失败: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for name in String::from_utf8_lossy(&output.stdout).lines() {
        let name = name.trim();
        if !name.is_empty() {
            std::env::remove_var(name);
        }
    }
    Ok(())
}

/// 单包门禁结果。
struct PackageOutcome {
    status: Option<ExitStatus>,
    timed_out: bool,
}

/// 以独立进程组跑单包测试，超时先 TERM 进程组、200ms 后 KILL。
fn run_package(
    repo_root: &Path,
    gate: &PackageGate,
    timeout: Duration,
    log_path: &Path,
) -> Result<PackageOutcome> {
    use std::os::unix::process::CommandExt;

    let log_file =
        fs::File::create(log_path).with_context(|| format!("创建 {} 失败", log_path.display()))?;
    let mut command = Command::new("cargo");
    command
        .arg("test")
        .arg("-p")
        .arg(gate.package)
        .args(gate.extra_args)
        .current_dir(repo_root)
        .stdout(Stdio::from(log_file.try_clone()?))
        .stderr(Stdio::from(log_file))
        // 独立进程组：超时收割必须覆盖 cargo 派生的全部测试子进程。
        .process_group(0);
    let mut child = command.spawn().context("spawn cargo test 失败")?;

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => {
                return Ok(PackageOutcome {
                    status: Some(status),
                    timed_out: false,
                });
            }
            None if Instant::now() >= deadline => {
                terminate_process_group(child.id());
                let _ = child.wait();
                return Ok(PackageOutcome {
                    status: None,
                    timed_out: true,
                });
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[cfg(unix)]
fn terminate_process_group(child_pid: u32) {
    // 子进程经 process_group(0) 自任组长，pid 即 pgid；负号按进程组投递。
    let pgid = child_pid as i32;
    unsafe {
        libc::kill(-pgid, libc::SIGTERM);
    }
    std::thread::sleep(Duration::from_millis(200));
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
}

fn exit_code_of(status: &ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    if let Some(signal) = status.signal() {
        128 + signal
    } else {
        status.code().unwrap_or(1)
    }
}

/// 失败日志摘要：错误相关行前 40 行（对应原脚本的 grep 摘要）。
fn failure_excerpt(log_path: &Path) -> String {
    let Ok(mut file) = fs::File::open(log_path) else {
        return String::new();
    };
    let mut content = String::new();
    let _ = file.read_to_string(&mut content);
    content
        .lines()
        .filter(|line| {
            line.contains("error[")
                || line.contains("error:")
                || line.contains("FAILED")
                || line.contains("panicked")
                || line.contains("failures:")
        })
        .take(40)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 成功摘要：最后一条 `test result:` 行。
fn success_summary(log_path: &Path) -> Option<String> {
    let content = fs::read_to_string(log_path).ok()?;
    content
        .lines()
        .rfind(|line| line.starts_with("test result:"))
        .map(str::to_owned)
}

/// 逐包跑测试门禁；任一失败立即返回（fail-fast），exit code 原样传播。
pub fn run(repo_root: &Path, timeout_secs: u64) -> Result<BTreeMap<String, String>> {
    strip_git_local_env(repo_root)?;

    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        std::env::set_var("CARGO_TARGET_DIR", "target/hook-tests");
    }
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap();
    let log_dir = PathBuf::from(&target_dir).join("hook-logs");
    fs::create_dir_all(&log_dir).context("创建 hook-logs 目录失败")?;

    let timeout = Duration::from_secs(timeout_secs);
    let mut summaries = BTreeMap::new();
    for gate in PACKAGE_GATES {
        let args_preview = format!(
            "cargo test -p {} {}",
            gate.package,
            gate.extra_args.join(" ")
        );
        println!("==> {args_preview} (timeout: {timeout_secs}s)");
        let log_path = log_dir.join(format!("{}.log", gate.package));
        let outcome = run_package(repo_root, gate, timeout, &log_path)?;
        if outcome.timed_out {
            eprintln!(
                "[hook-timeout] package {} exceeded {}s",
                gate.package, timeout_secs
            );
            return Err(GateFailure {
                exit_code: 124,
                message: format!("包 {} 测试超过 {}s 限时", gate.package, timeout_secs),
            }
            .into());
        }
        let status = outcome.status.expect("非超时必有退出状态");
        if !status.success() {
            let code = exit_code_of(&status);
            eprintln!(
                "[hook] {} FAILED (rc={code}); 完整日志: {}",
                gate.package,
                log_path.display()
            );
            let excerpt = failure_excerpt(&log_path);
            if !excerpt.is_empty() {
                eprintln!("{excerpt}");
            }
            return Err(GateFailure {
                exit_code: code,
                message: format!("包 {} 测试失败（rc={code}）", gate.package),
            }
            .into());
        }
        let summary = success_summary(&log_path)
            .unwrap_or_else(|| format!("[hook] {}: (no test result line)", gate.package));
        println!("{summary}\n");
        summaries.insert(gate.package.to_owned(), summary);
    }
    Ok(summaries)
}
