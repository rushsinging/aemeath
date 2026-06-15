use super::*;
use serde_json::json;

#[test]
fn sink_paths_in_logs_dir() {
    let paths = SinkPaths::from_logs_dir(Path::new("/tmp/logs"));
    assert_eq!(paths.aemeath, PathBuf::from("/tmp/logs/aemeath.log"));
    assert_eq!(paths.runtime, PathBuf::from("/tmp/logs/runtime.log"));
    assert_eq!(paths.provider, PathBuf::from("/tmp/logs/provider.log"));
    assert_eq!(paths.tools, PathBuf::from("/tmp/logs/tools.log"));
    assert_eq!(paths.prompt, PathBuf::from("/tmp/logs/prompt.log"));
    assert_eq!(paths.tui, PathBuf::from("/tmp/logs/tui.log"));
    assert_eq!(paths.hook, PathBuf::from("/tmp/logs/hook.log"));
    assert_eq!(paths.input, PathBuf::from("/tmp/logs/input.log"));
    assert_eq!(paths.output, PathBuf::from("/tmp/logs/output.log"));
    assert_eq!(paths.audit, PathBuf::from("/tmp/logs/audit.log"));
}

#[test]
fn static_audit_methods_are_noop_without_init() {
    // 未 init 时 log_input/output/user_input/audit 应静默 no-op（不能 panic）
    UnifiedLogger::log_input("default", json!({}));
    UnifiedLogger::log_output("default", json!({}));
    UnifiedLogger::log_user_input(json!({}));
    UnifiedLogger::audit("permission", json!({}));
}

/// 构造一个仅用于测试 `maybe_rotate` 的最小 logger（其余 sink 留空）。
fn rotate_test_logger(dir: &Path, max_bytes: u64, max_backups: usize) -> UnifiedLogger {
    UnifiedLogger {
        aemeath: Mutex::new(None),
        runtime: Mutex::new(None),
        provider: Mutex::new(None),
        tools: Mutex::new(None),
        prompt: Mutex::new(None),
        tui: Mutex::new(None),
        hook: Mutex::new(None),
        input: Mutex::new(None),
        output: Mutex::new(None),
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
fn route_returns_correct_sink_for_known_prefixes() {
    let logger = rotate_test_logger(&std::env::temp_dir(), 1024, 3);

    // Verify each prefix routes to the correct path
    let (_, path_cli) = logger.route("cli::tui::render").unwrap();
    assert_eq!(path_cli, &logger.paths.tui);

    let (_, path_hook) = logger.route("hook::runner").unwrap();
    assert_eq!(path_hook, &logger.paths.hook);

    let (_, path_runtime) = logger.route("runtime::loop_runner").unwrap();
    assert_eq!(path_runtime, &logger.paths.runtime);

    let (_, path_provider) = logger.route("provider::client").unwrap();
    assert_eq!(path_provider, &logger.paths.provider);

    let (_, path_tools) = logger.route("tools::mcp").unwrap();
    assert_eq!(path_tools, &logger.paths.tools);

    let (_, path_prompt) = logger.route("prompt::guidance").unwrap();
    assert_eq!(path_prompt, &logger.paths.prompt);
}

#[test]
fn route_returns_none_for_unknown_prefix() {
    let logger = rotate_test_logger(&std::env::temp_dir(), 1024, 3);
    assert!(logger.route("unknown::module").is_none());
    assert!(logger.route("app").is_none());
}
