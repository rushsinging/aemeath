//! 任务日志文件（#1890 输出直绑）测试：路径推导、创建、终态兜底 append、
//! 区间读游标、快路径删除、session 级清理。

use super::*;

fn temp_base() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bgp-log-tests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn process_id() -> crate::domain::background_process::BackgroundProcessId {
    crate::domain::background_process::BackgroundProcessId::new_v7()
}

#[test]
fn session_logs_dir_uses_sidecar_layout_under_base() {
    let base = temp_base();
    let dir = session_logs_dir(&base, "sess-1");
    assert_eq!(
        dir,
        base.join("sess-1.background-process"),
        "平铺前缀式目录：不打破 sessions 既有平铺布局"
    );
}

#[test]
fn log_path_derives_from_session_and_process_id() {
    let base = temp_base();
    let process_id = process_id();
    let log = TaskLogFile::path_for(&base, "sess-1", &process_id);
    assert_eq!(
        log,
        base.join("sess-1.background-process")
            .join(format!("{}.log", process_id.as_str()))
    );
}

#[test]
fn open_creates_file_and_returns_append_handles() {
    let base = temp_base();
    let process_id = process_id();
    let (log, mut stdout, mut stderr) = TaskLogFile::open(&base, "sess-1", &process_id).unwrap();
    assert!(log.path().exists(), "open 即建文件（派发即创建）");
    use std::io::Write as _;
    stdout.write_all(b"out\n").unwrap();
    stderr.write_all(b"err\n").unwrap();
    // 双 handle 各自 O_APPEND 追加，无覆盖。
    let content = std::fs::read(log.path()).unwrap();
    assert_eq!(content.len(), 8, "stdout/stderr 都落盘，无互相覆盖");
}

#[test]
fn read_range_returns_bytes_and_advances_cursor() {
    let base = temp_base();
    let process_id = process_id();
    let (log, mut stdout, _stderr) = TaskLogFile::open(&base, "sess-1", &process_id).unwrap();
    use std::io::Write as _;
    stdout.write_all(b"0123456789").unwrap();
    drop(stdout);

    let first = log.read_range(0, 4).unwrap();
    assert_eq!(first.bytes, b"0123".to_vec());
    assert_eq!(first.next_cursor, 4);

    let rest = log.read_range(first.next_cursor, usize::MAX).unwrap();
    assert_eq!(rest.bytes, b"456789".to_vec());
    assert_eq!(rest.next_cursor, 10, "游标推进到文件尾");

    let beyond = log.read_range(rest.next_cursor, 100).unwrap();
    assert!(beyond.bytes.is_empty(), "游标在文件尾时空段");
}

#[test]
fn read_range_reports_truncation_when_capped() {
    let base = temp_base();
    let process_id = process_id();
    let (log, mut stdout, _) = TaskLogFile::open(&base, "sess-1", &process_id).unwrap();
    use std::io::Write as _;
    stdout.write_all(b"0123456789").unwrap();
    drop(stdout);

    let segment = log.read_range(0, 4).unwrap();
    assert!(segment.truncated, "max_bytes 截断时置位");
}

#[test]
fn size_bytes_reflects_file_content() {
    let base = temp_base();
    let process_id = process_id();
    let (log, mut stdout, _) = TaskLogFile::open(&base, "sess-1", &process_id).unwrap();
    assert_eq!(log.size_bytes(), 0);
    use std::io::Write as _;
    stdout.write_all(b"abc").unwrap();
    drop(stdout);
    assert_eq!(log.size_bytes(), 3, "total_written_bytes 取文件实际大小");
}

#[test]
fn append_terminal_appends_once_per_call() {
    let base = temp_base();
    let process_id = process_id();
    let (log, _, _) = TaskLogFile::open(&base, "sess-1", &process_id).unwrap();
    log.append_terminal("result-a\n").unwrap();
    log.append_terminal("result-b\n").unwrap();
    let content = std::fs::read(log.path()).unwrap();
    assert_eq!(content, b"result-a\nresult-b\n".to_vec());
}

#[test]
fn remove_deletes_file_for_fast_path() {
    let base = temp_base();
    let process_id = process_id();
    let (log, _, _) = TaskLogFile::open(&base, "sess-1", &process_id).unwrap();
    log.remove().unwrap();
    assert!(!log.path().exists(), "快路径完成后不留垃圾文件");
}

#[test]
fn remove_session_logs_clears_whole_session_namespace() {
    let base = temp_base();
    for _ in 0..3 {
        let process_id = process_id();
        let _ = TaskLogFile::open(&base, "sess-1", &process_id).unwrap();
    }
    let _ = TaskLogFile::open(&base, "sess-2", &process_id()).unwrap();

    remove_session_logs(&base, "sess-1").unwrap();

    assert!(
        !base.join("sess-1.background-process").exists(),
        "整个 session 目录清除"
    );
    assert!(
        base.join("sess-2.background-process").exists(),
        "其他 session 不受影响"
    );
}
