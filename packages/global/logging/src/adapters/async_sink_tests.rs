use super::*;
use crate::adapters::lifecycle::{FileMetadata, FileOps, MonotonicClock, SinkWriter, StdFileOps};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// 内存 fake 文件系统：write_all 人工延迟，用于构造"worker 写得慢"场景。
struct SlowMemoryFiles {
    write_delay: Duration,
    output: Mutex<Vec<u8>>,
    metadata_len: AtomicU64,
}

impl SlowMemoryFiles {
    fn new(write_delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            write_delay,
            output: Mutex::new(Vec::new()),
            metadata_len: AtomicU64::new(0),
        })
    }

    fn written_text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }
}

impl SinkWriter for SlowWriter {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        std::thread::sleep(self.files.write_delay);
        self.files.output.lock().unwrap().extend_from_slice(bytes);
        self.files
            .metadata_len
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct SlowWriter {
    files: Arc<SlowMemoryFiles>,
}

/// Arc-aware adapter：`open` 需要返回共享同一 fake 的 writer。
struct SlowMemoryFileOps(Arc<SlowMemoryFiles>);

impl FileOps for SlowMemoryFileOps {
    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn SinkWriter>> {
        Ok(Box::new(SlowWriter {
            files: Arc::clone(&self.0),
        }))
    }

    fn metadata(&self, _path: &Path) -> std::io::Result<FileMetadata> {
        Ok(FileMetadata {
            len: self.0.metadata_len.load(Ordering::Relaxed),
            is_file: true,
            is_symlink: false,
            modified: SystemTime::now(),
        })
    }

    fn exists(&self, _path: &Path) -> std::io::Result<bool> {
        Ok(true)
    }

    fn remove(&self, _path: &Path) -> std::io::Result<()> {
        Ok(())
    }

    fn rename(&self, _from: &Path, _to: &Path) -> std::io::Result<()> {
        Ok(())
    }

    fn read_dir(&self, _path: &Path) -> std::io::Result<Vec<PathBuf>> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct RealClock;

impl MonotonicClock for RealClock {
    fn now(&self) -> Duration {
        Duration::from_secs(0)
    }
}

#[derive(Default)]
struct RecordingEmergency {
    messages: Mutex<Vec<String>>,
}

impl EmergencyWriter for RecordingEmergency {
    fn write(&self, message: &str) {
        self.messages.lock().unwrap().push(message.to_string());
    }
}

fn slow_lifecycle(
    files: &Arc<SlowMemoryFiles>,
    emergency: Arc<RecordingEmergency>,
) -> FileSinkLifecycle {
    FileSinkLifecycle::start(
        PathBuf::from("/logs/aemeath.log"),
        1024 * 1024 * 1024,
        5,
        0,
        Arc::new(SlowMemoryFileOps(Arc::clone(files))),
        Arc::new(RealClock),
        emergency,
    )
}

fn single_sink_worker(
    files: &Arc<SlowMemoryFiles>,
    emergency: Arc<RecordingEmergency>,
    capacity: usize,
) -> AsyncSinkWorker {
    let lifecycle = slow_lifecycle(files, emergency.clone());
    let mut lifecycles = HashMap::new();
    lifecycles.insert(DiagnosticSinkId::Fallback, lifecycle);
    AsyncSinkWorker::spawn(lifecycles, emergency as Arc<dyn EmergencyWriter>, capacity)
}

#[test]
fn enqueue_returns_promptly_when_channel_is_saturated() {
    let files = SlowMemoryFiles::new(Duration::from_millis(20));
    let emergency = Arc::new(RecordingEmergency::default());
    let worker = single_sink_worker(&files, emergency.clone(), 1);
    let handle = worker.handle();

    // 先塞满容量为 1 的 channel，之后所有 enqueue 都应立即丢弃返回。
    handle.enqueue_line(DiagnosticSinkId::Fallback, "warm-up".to_string());
    let started_at = Instant::now();
    for index in 0..20 {
        handle.enqueue_line(DiagnosticSinkId::Fallback, format!("line-{index}"));
    }
    let elapsed = started_at.elapsed();

    // 20 行 × 20ms/行的同步写需要 ≥400ms；异步 enqueue 必须远小于该值。
    assert!(
        elapsed < Duration::from_millis(200),
        "enqueue 被反压：20 次入队耗时 {elapsed:?}"
    );
    assert!(handle.dropped_lines() > 0, "饱和丢弃必须被计数");

    worker.join();
}

#[test]
fn flush_barrier_writes_all_previously_enqueued_lines() {
    let files = SlowMemoryFiles::new(Duration::from_millis(0));
    let emergency = Arc::new(RecordingEmergency::default());
    let worker = single_sink_worker(&files, emergency.clone(), 64);
    let handle = worker.handle();

    for index in 0..10 {
        handle.enqueue_line(DiagnosticSinkId::Fallback, format!("persisted-{index}"));
    }
    handle.flush_barrier();

    let written = files.written_text();
    for index in 0..10 {
        assert!(
            written.contains(&format!("persisted-{index}")),
            "flush barrier 返回后必须已落盘：{written}"
        );
    }
    worker.join();
}

#[test]
fn flush_barrier_reports_and_resets_dropped_counter() {
    let files = SlowMemoryFiles::new(Duration::from_millis(20));
    let emergency = Arc::new(RecordingEmergency::default());
    let worker = single_sink_worker(&files, emergency.clone(), 1);
    let handle = worker.handle();

    for index in 0..10 {
        handle.enqueue_line(DiagnosticSinkId::Fallback, format!("burst-{index}"));
    }
    let dropped_before = handle.dropped_lines();
    handle.flush_barrier();

    assert_eq!(handle.dropped_lines(), 0, "flush 后丢弃计数应归零");
    let messages = emergency.messages.lock().unwrap().join("\n");
    assert!(
        messages.contains(&format!("dropped={dropped_before}")),
        "丢弃行数必须写入 emergency 报告：{messages}"
    );
    worker.join();
}

#[test]
fn join_returns_lifecycles_after_channel_closes() {
    let files = SlowMemoryFiles::new(Duration::from_millis(0));
    let emergency = Arc::new(RecordingEmergency::default());
    let worker = single_sink_worker(&files, emergency.clone(), 4);
    worker
        .handle()
        .enqueue_line(DiagnosticSinkId::Fallback, "before-close".to_string());

    // join 消耗 worker 并 drop 内部 sender，worker 排空剩余命令后退出。
    let lifecycles = worker.join();
    assert!(lifecycles.contains_key(&DiagnosticSinkId::Fallback));
    assert!(files.written_text().contains("before-close"));
}

#[test]
fn real_files_are_written_by_worker_thread() {
    // 端到端：真实文件系统 + StdFileOps，验证生产装配路径下异步写入最终落盘。
    let temp_dir = std::env::temp_dir().join(format!(
        "aemeath-async-sink-e2e-{}-{:?}",
        std::process::id(),
        SystemTime::now()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let log_path = temp_dir.join("aemeath.log");
    let emergency = Arc::new(RecordingEmergency::default());
    let lifecycle = FileSinkLifecycle::start(
        log_path.clone(),
        1024 * 1024 * 1024,
        5,
        0,
        Arc::new(StdFileOps),
        Arc::new(RealClock),
        emergency.clone() as Arc<dyn EmergencyWriter>,
    );
    let mut lifecycles = HashMap::new();
    lifecycles.insert(DiagnosticSinkId::Fallback, lifecycle);
    let worker = AsyncSinkWorker::spawn(lifecycles, emergency as Arc<dyn EmergencyWriter>, 64);
    worker
        .handle()
        .enqueue_line(DiagnosticSinkId::Fallback, "real-file-line".to_string());
    worker.handle().flush_barrier();

    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("real-file-line"));
    worker.join();
    let _ = std::fs::remove_dir_all(&temp_dir);
}
