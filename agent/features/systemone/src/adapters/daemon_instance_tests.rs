//! Daemon 单实例锁与 sock 路径解析测试。

use super::*;
use std::os::unix::net::UnixListener;
use std::path::Path;

#[test]
fn endpoints_under_agents_dir_use_scoring_sock_and_lock() {
    let endpoints = DaemonEndpoints::under_agents_dir(Path::new("/tmp/demo-agents"));
    assert_eq!(
        endpoints.sock,
        Path::new("/tmp/demo-agents/scoring/scoring.sock")
    );
    assert_eq!(
        endpoints.lock,
        Path::new("/tmp/demo-agents/scoring/scoring.lock")
    );
    assert_eq!(endpoints.scoring_dir, Path::new("/tmp/demo-agents/scoring"));
}

#[test]
fn endpoints_honor_socket_override_but_keep_lock_under_scoring_dir() {
    let endpoints = DaemonEndpoints::under_agents_dir_with_socket(
        Path::new("/tmp/demo-agents"),
        Some(Path::new("/tmp/custom/my.sock").to_path_buf()),
    );
    assert_eq!(endpoints.sock, Path::new("/tmp/custom/my.sock"));
    assert_eq!(
        endpoints.lock,
        Path::new("/tmp/demo-agents/scoring/scoring.lock")
    );
}

#[test]
fn second_lock_acquire_fails_while_first_guard_lives() {
    let temp = tempfile::tempdir().expect("tempdir");
    let lock_path = temp.path().join("scoring.lock");

    let first = acquire_daemon_lock(&lock_path).expect("首次加锁应成功");
    let second = acquire_daemon_lock(&lock_path);
    assert!(matches!(second, Err(DaemonLockError::AlreadyRunning)));
    drop(first);

    let third = acquire_daemon_lock(&lock_path).expect("释放后应可再加锁");
    drop(third);
}

#[test]
fn prepare_socket_unlinks_stale_sock_when_lock_held() {
    let temp = tempfile::tempdir().expect("tempdir");
    let sock_path = temp.path().join("scoring.sock");
    let lock_path = temp.path().join("scoring.lock");

    // 制造陈旧 sock（bind 后 drop 监听者，路径仍在）。
    let listener = UnixListener::bind(&sock_path).expect("bind");
    drop(listener);
    assert!(sock_path.exists());

    let guard = acquire_daemon_lock(&lock_path).expect("加锁");
    prepare_socket_path(&sock_path, &guard).expect("持锁应可 unlink 陈旧 sock");
    assert!(!sock_path.exists());
    // 清理后应可再次 bind。
    let listener = UnixListener::bind(&sock_path).expect("re-bind");
    drop(listener);
    drop(guard);
}

#[test]
fn acquire_daemon_lock_creates_parent_scoring_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let lock_path = temp.path().join("scoring").join("scoring.lock");
    assert!(!lock_path.parent().expect("parent").exists());
    let guard = acquire_daemon_lock(&lock_path).expect("应创建父目录并加锁");
    assert!(lock_path.exists());
    drop(guard);
}
