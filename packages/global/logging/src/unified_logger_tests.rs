use super::*;
use serde_json::json;

#[test]
fn sink_paths_in_logs_dir() {
    let paths = SinkPaths::from_logs_dir(Path::new("/tmp/logs"));
    assert_eq!(paths.aemeath, PathBuf::from("/tmp/logs/aemeath.log"));
    assert_eq!(paths.tui, PathBuf::from("/tmp/logs/tui.log"));
    assert_eq!(paths.shared, PathBuf::from("/tmp/logs/shared.log"));
    assert_eq!(paths.composition, PathBuf::from("/tmp/logs/composition.log"));
    assert_eq!(paths.provider, PathBuf::from("/tmp/logs/agent-provider.log"));
    assert_eq!(paths.runtime, PathBuf::from("/tmp/logs/agent-runtime.log"));
    assert_eq!(paths.tools, PathBuf::from("/tmp/logs/agent-tools.log"));
    assert_eq!(paths.prompt, PathBuf::from("/tmp/logs/agent-prompt.log"));
    assert_eq!(paths.hook, PathBuf::from("/tmp/logs/agent-hook.log"));
    assert_eq!(paths.storage, PathBuf::from("/tmp/logs/agent-storage.log"));
    assert_eq!(paths.project, PathBuf::from("/tmp/logs/agent-project.log"));
    assert_eq!(paths.policy, PathBuf::from("/tmp/logs/agent-policy.log"));
    assert_eq!(paths.audit, PathBuf::from("/tmp/logs/agent-audit.log"));
}

#[test]
fn static_audit_methods_are_noop_without_init() {
    // 未 init 时 log_input/output/user_input 应静默 no-op（不能 panic）
    UnifiedLogger::log_input("default", json!({}));
    UnifiedLogger::log_output("default", json!({}));
    UnifiedLogger::log_user_input(json!({}));
}

/// 构造一个仅用于测试 `maybe_rotate` 的最小 logger（其余 sink 留空）。
fn rotate_test_logger(dir: &Path, max_bytes: u64, max_backups: usize) -> UnifiedLogger {
    UnifiedLogger {
        aemeath: Mutex::new(None),
        tui: Mutex::new(None),
        shared: Mutex::new(None),
        composition: Mutex::new(None),
        provider: Mutex::new(None),
        runtime: Mutex::new(None),
        tools: Mutex::new(None),
        prompt: Mutex::new(None),
        hook: Mutex::new(None),
        storage: Mutex::new(None),
        project: Mutex::new(None),
        policy: Mutex::new(None),
        audit: Mutex::new(None),
        paths: SinkPaths::from_logs_dir(dir),
        max_bytes,
        max_backups,
        role_logs_enabled: false,
        filter: build_filter(LevelFilter::Off),
    }
}

// 正常路径：达到阈值时轮转，并经 guard 重装新 writer（不重入 sink 锁、不死锁）。
#[test]
fn maybe_rotate_rotates_and_reinstalls_writer_when_over_threshold() {
    let dir = std::env::temp_dir().join("aem_rot_over_threshold");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("aemeath.log");
    File::create(&path)
        .unwrap()
        .write_all(&[b'x'; 2048])
        .unwrap();

    let logger = rotate_test_logger(&dir, 1024, 3);
    let sink: Mutex<Option<BufWriter<File>>> = Mutex::new(Some(open_buf(&path).unwrap()));
    {
        // 持锁状态下调用 —— 旧实现会在此对同一把锁重入而自死锁。
        let mut guard = sink.lock().unwrap();
        logger.maybe_rotate(&path, &mut guard);
        assert!(guard.is_some(), "轮转后应经 guard 安装新 writer");
        writeln!(guard.as_mut().unwrap(), "fresh").unwrap();
        guard.as_mut().unwrap().flush().unwrap();
    }
    assert!(dir.join("aemeath.log.1").exists(), "旧内容应轮转到 .1");
    let new_len = fs::metadata(&path).unwrap().len();
    assert!(new_len < 1024, "新文件应只含 fresh 行，实际 {new_len} 字节");
    let _ = fs::remove_dir_all(&dir);
}

// 边界：未达阈值时不轮转、不动 writer。
#[test]
fn maybe_rotate_is_noop_when_under_threshold() {
    let dir = std::env::temp_dir().join("aem_rot_under_threshold");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("aemeath.log");
    File::create(&path)
        .unwrap()
        .write_all(&[b'x'; 100])
        .unwrap();

    let logger = rotate_test_logger(&dir, 1024, 3);
    let sink: Mutex<Option<BufWriter<File>>> = Mutex::new(Some(open_buf(&path).unwrap()));
    {
        let mut guard = sink.lock().unwrap();
        logger.maybe_rotate(&path, &mut guard);
        assert!(guard.is_some(), "未达阈值不应清空 writer");
    }
    assert!(!dir.join("aemeath.log.1").exists(), "未达阈值不应轮转");
    let _ = fs::remove_dir_all(&dir);
}

// 错误路径：目标文件不存在时 metadata 失败，直接 no-op，不创建文件/writer。
#[test]
fn maybe_rotate_is_noop_when_file_missing() {
    let dir = std::env::temp_dir().join("aem_rot_missing");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("does_not_exist.log");

    let logger = rotate_test_logger(&dir, 1024, 3);
    let mut guard: Option<BufWriter<File>> = None;
    logger.maybe_rotate(&path, &mut guard);
    assert!(guard.is_none(), "缺文件应直接 no-op");
    assert!(!path.exists(), "no-op 不应创建文件");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn route_returns_correct_sink_for_aemeath_targets() {
    let logger = rotate_test_logger(&std::env::temp_dir(), 1024, 3);

    let (_, path) = logger.route("aemeath:tui");
    assert_eq!(path, &logger.paths.tui);

    let (_, path) = logger.route("aemeath:shared");
    assert_eq!(path, &logger.paths.shared);

    let (_, path) = logger.route("aemeath:composition");
    assert_eq!(path, &logger.paths.composition);

    let (_, path) = logger.route("aemeath:agent:provider");
    assert_eq!(path, &logger.paths.provider);

    let (_, path) = logger.route("aemeath:agent:runtime");
    assert_eq!(path, &logger.paths.runtime);

    let (_, path) = logger.route("aemeath:agent:tools");
    assert_eq!(path, &logger.paths.tools);

    let (_, path) = logger.route("aemeath:agent:prompt");
    assert_eq!(path, &logger.paths.prompt);

    let (_, path) = logger.route("aemeath:agent:hook");
    assert_eq!(path, &logger.paths.hook);

    let (_, path) = logger.route("aemeath:agent:storage");
    assert_eq!(path, &logger.paths.storage);

    let (_, path) = logger.route("aemeath:agent:project");
    assert_eq!(path, &logger.paths.project);

    let (_, path) = logger.route("aemeath:agent:policy");
    assert_eq!(path, &logger.paths.policy);

    let (_, path) = logger.route("aemeath:agent:audit");
    assert_eq!(path, &logger.paths.audit);
}

#[test]
fn route_returns_aemeath_for_unknown_target() {
    let logger = rotate_test_logger(&std::env::temp_dir(), 1024, 3);
    let (_, path) = logger.route("unknown::module");
    assert_eq!(path, &logger.paths.aemeath);
    let (_, path) = logger.route("app");
    assert_eq!(path, &logger.paths.aemeath);
}
