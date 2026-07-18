use super::*;

#[test]
fn sink_paths_in_logs_dir() {
    let logger = rotate_test_logger(Path::new("/tmp/logs"), 1024, 3);
    for spec in TargetCatalog::specs() {
        let entry = logger.sinks.get(&spec.sink).expect("catalog sink");
        assert_eq!(entry.path, PathBuf::from("/tmp/logs").join(spec.file_name));
    }
    let fallback = TargetCatalog::fallback();
    assert_eq!(
        logger.sinks.get(&fallback.sink).expect("fallback").path,
        PathBuf::from("/tmp/logs/aemeath.log")
    );
}

/// 构造一个仅用于测试 `maybe_rotate` 的最小 logger。
fn rotate_test_logger(dir: &Path, max_bytes: u64, max_backups: usize) -> UnifiedLogger {
    let mut sinks = HashMap::new();
    let fallback = TargetCatalog::fallback();
    sinks.insert(
        fallback.sink,
        SinkEntry {
            path: dir.join(fallback.file_name),
            writer: Mutex::new(None),
        },
    );
    for spec in TargetCatalog::specs() {
        sinks.insert(
            spec.sink,
            SinkEntry {
                path: dir.join(spec.file_name),
                writer: Mutex::new(None),
            },
        );
    }
    let settings = LoggingSettings::new(
        "off".to_string(),
        LoggingOutputMode::File,
        dir.to_path_buf(),
        max_bytes,
        max_backups,
        30,
    );
    UnifiedLogger {
        sinks,
        stderr: Mutex::new(BufWriter::new(stderr())),
        output_mode: settings.output_mode(),
        max_bytes: settings.max_bytes(),
        max_backups: settings.max_backups(),
        filter: build_filter(settings.filter_directive()),
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
    for spec in TargetCatalog::specs() {
        let (_, path) = logger.route(spec.target.as_str());
        assert_eq!(
            path,
            logger
                .sinks
                .get(&spec.sink)
                .map(|entry| entry.path.as_path())
                .unwrap()
        );
    }
}

#[test]
fn route_returns_aemeath_for_unknown_target() {
    let logger = rotate_test_logger(&std::env::temp_dir(), 1024, 3);
    let fallback_path = &logger
        .sinks
        .get(&TargetCatalog::fallback().sink)
        .expect("fallback")
        .path;
    let (_, path) = logger.route("unknown::module");
    assert_eq!(path, fallback_path);
    let (_, path) = logger.route("app");
    assert_eq!(path, fallback_path);
}

#[test]
fn unknown_target_reports_are_limited_and_written_directly() {
    let counter = AtomicUsize::new(0);
    assert!(should_report_unknown(&counter));
    assert!(should_report_unknown(&counter));
    assert!(should_report_unknown(&counter));
    assert!(!should_report_unknown(&counter));

    let mut output = Vec::new();
    write_unknown_target_report(&mut output, "unknown::module").unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "aemeath logging fallback: unknown target \"unknown::module\"; using aemeath.log\n"
    );
}
