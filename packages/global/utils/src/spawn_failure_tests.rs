use super::{describe_cwd_gone, describe_cwd_gone_failure};
use std::fs;
use std::io;
use std::path::PathBuf;

fn unique_temp_dir(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "aemeath_utils_spawn_failure_{name}_{}_{id}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path.canonicalize().unwrap()
}

#[test]
fn describe_when_cwd_gone_and_not_found_returns_attribution() {
    let tempdir = unique_temp_dir("cwd_gone");
    fs::remove_dir_all(&tempdir).unwrap();
    let error = io::Error::new(
        io::ErrorKind::NotFound,
        "No such file or directory (os error 2)",
    );

    let message = describe_cwd_gone_failure(&error, &tempdir, "Bash 命令")
        .expect("cwd 缺失且 NotFound 时必须给出归因文案");

    assert!(
        message.contains(&tempdir.display().to_string()),
        "文案必须包含缺失路径，实际: {message}"
    );
    assert!(
        message.contains("Bash 命令"),
        "文案必须包含业务主体，实际: {message}"
    );
    assert!(
        message.contains("可能原因"),
        "文案必须给出可能原因，实际: {message}"
    );
    assert!(
        message.contains("建议"),
        "文案必须给出建议动作，实际: {message}"
    );
}

#[test]
fn describe_when_cwd_exists_returns_none() {
    let tempdir = unique_temp_dir("cwd_exists");
    // NotFound 但目录完好：失败源是可执行文件缺失等其他原因，不得误归因到 cwd。
    let error = io::Error::new(
        io::ErrorKind::NotFound,
        "No such file or directory (os error 2)",
    );

    assert!(
        describe_cwd_gone_failure(&error, &tempdir, "hook 命令").is_none(),
        "cwd 存在时不得归因到目录缺失"
    );

    fs::remove_dir_all(&tempdir).unwrap();
}

#[test]
fn describe_when_error_kind_other_than_not_found_returns_none() {
    let tempdir = unique_temp_dir("kind_other");
    fs::remove_dir_all(&tempdir).unwrap();
    let error = io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Permission denied (os error 13)",
    );

    assert!(
        describe_cwd_gone_failure(&error, &tempdir, "搜索命令").is_none(),
        "非 NotFound 错误即使 cwd 缺失也不得归因到目录缺失"
    );
}

#[test]
fn describe_gone_when_cwd_deleted_returns_attribution_without_error() {
    let tempdir = unique_temp_dir("pure_cwd_gone");
    fs::remove_dir_all(&tempdir).unwrap();

    let message =
        describe_cwd_gone(&tempdir, "git 上下文").expect("cwd 缺失时纯检查必须给出归因文案");

    assert!(message.contains(&tempdir.display().to_string()));
    assert!(message.contains("git 上下文"));
}

#[test]
fn describe_gone_when_cwd_exists_returns_none() {
    let tempdir = unique_temp_dir("pure_cwd_alive");

    assert!(
        describe_cwd_gone(&tempdir, "git 上下文").is_none(),
        "cwd 存在时不得归因"
    );

    fs::remove_dir_all(&tempdir).unwrap();
}
