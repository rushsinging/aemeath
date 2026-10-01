use super::*;

// env var 是进程级状态，并行测试不安全，合并到一个测试函数串行运行。
#[test]
fn session_lock_lifecycle() {
    // ===== case 1: acquire → release =====
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("AEMEATH_AGENTS_DIR", tmp.path());
    let id = "test-acquire-release";

    let mut lock = acquire(id).expect("first acquire should succeed");
    assert!(lock.path.exists(), "lock file should exist after acquire");
    release(&mut lock).expect("release should succeed");
    assert!(
        !lock.path.exists(),
        "lock file should be removed after release"
    );

    // ===== case 2: drop releases =====
    let id = "test-drop-release";
    let path;
    {
        let lock = acquire(id).expect("acquire should succeed");
        path = lock.path.clone();
        assert!(path.exists());
    }
    assert!(!path.exists(), "lock file should be removed on drop");

    // ===== case 3: alive pid blocks =====
    let id = "test-second-acquire";
    let path = lock_path(id);
    let meta = SessionLockMeta {
        pid: std::process::id(),
        created_at: now_iso(),
        hostname: "test".to_string(),
    };
    write_meta(&path, &meta).unwrap();

    let err = acquire(id).expect_err("alive pid should block");
    match err {
        LockError::HeldAlive { pid, .. } => {
            assert_eq!(pid, std::process::id());
        }
        other => panic!("expected HeldAlive, got {other:?}"),
    }
    let _ = fs::remove_file(&path);

    // ===== case 4: dead pid taken over =====
    // 用 fork 创建一个立即退出的子进程，拿到一个确定已死的 pid。
    let dead_pid = {
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            std::process::exit(0);
        }
        // 等待子进程退出
        unsafe {
            libc::waitpid(pid, std::ptr::null_mut(), 0);
        }
        pid as u32
    };
    let id = "test-dead-pid";
    let path = lock_path(id);
    let meta = SessionLockMeta {
        pid: dead_pid,
        created_at: now_iso(),
        hostname: "test".to_string(),
    };
    write_meta(&path, &meta).unwrap();
    let lock = acquire(id).expect("dead pid should be taken over");
    assert_eq!(lock.path, path);
    let _ = fs::remove_file(&path);

    // ===== case 5: force_acquire overrides =====
    let id = "test-force";
    let path = lock_path(id);
    let meta = SessionLockMeta {
        pid: std::process::id(),
        created_at: now_iso(),
        hostname: "old".to_string(),
    };
    write_meta(&path, &meta).unwrap();
    let lock = force_acquire(id).expect("force should succeed");
    let new_meta: SessionLockMeta =
        serde_json::from_str(&fs::read_to_string(&lock.path).unwrap()).unwrap();
    assert_eq!(new_meta.hostname, hostname());
    let _ = fs::remove_file(&path);
}
