use super::*;
use std::io::Write;

/// 每个测试用独立临时文件，避免并行执行互相干扰。
fn temp_bin(tag: &str) -> PathBuf {
    let unique = format!(
        "aemeath-local-bin-watch-{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    std::fs::write(&path, b"old-binary").expect("create temp binary");
    path
}

fn overwrite(path: &Path, contents: &[u8]) {
    let mut file = std::fs::File::create(path).expect("rewrite temp binary");
    file.write_all(contents).expect("write temp binary");
    file.flush().expect("flush temp binary");
}

#[test]
fn poll_ignores_unchanged_binary() {
    let path = temp_bin("unchanged");
    let mut watch = LocalBinWatch::new(Some(path.clone()));
    let start = Instant::now();

    assert!(watch.poll(start + LOCAL_BIN_CHECK_INTERVAL).is_none());
    assert!(
        watch.poll(start + LOCAL_BIN_CHECK_INTERVAL * 2).is_none(),
        "文件未变化时不得提示"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn poll_detects_replacement_and_updates_baseline() {
    let path = temp_bin("changed");
    let mut watch = LocalBinWatch::new(Some(path.clone()));
    let start = Instant::now();

    overwrite(&path, b"new-binary-with-different-size");
    assert_eq!(
        watch.poll(start + LOCAL_BIN_CHECK_INTERVAL),
        Some(path.clone()),
        "文件被替换后必须返回检测到的路径"
    );
    assert!(
        watch.poll(start + LOCAL_BIN_CHECK_INTERVAL * 2).is_none(),
        "同一次替换只提醒一次"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn poll_is_throttled_between_checks() {
    let path = temp_bin("throttled");
    let mut watch = LocalBinWatch::new(Some(path.clone()));
    let start = Instant::now();

    assert!(watch.poll(start).is_none());
    overwrite(&path, b"new-binary-with-different-size");
    assert!(
        watch.poll(start + LOCAL_BIN_CHECK_INTERVAL / 2).is_none(),
        "未到检查间隔不得 stat"
    );
    assert_eq!(
        watch.poll(start + LOCAL_BIN_CHECK_INTERVAL),
        Some(path.clone()),
        "间隔到达后必须检测到变化"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn missing_path_is_silently_ignored() {
    let mut watch = LocalBinWatch::new(None);
    assert!(watch.poll(Instant::now()).is_none());

    let missing = std::env::temp_dir().join("aemeath-local-bin-watch-does-not-exist");
    let _ = std::fs::remove_file(&missing);
    let mut watch = LocalBinWatch::new(Some(missing));
    assert!(watch.poll(Instant::now()).is_none());
}
