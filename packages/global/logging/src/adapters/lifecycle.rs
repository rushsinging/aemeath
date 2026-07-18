//! File sink lifecycle, rotation, recovery, and retention.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub(super) const RECOVERY_INTERVAL: Duration = Duration::from_secs(5);

pub(super) trait SinkWriter: Send {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
}

pub(super) trait FileOps: Send + Sync {
    fn open(&self, path: &Path) -> io::Result<Box<dyn SinkWriter>>;
    fn metadata(&self, path: &Path) -> io::Result<FileMetadata>;
    fn exists(&self, path: &Path) -> io::Result<bool>;
    fn remove(&self, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
}

pub(super) trait MonotonicClock: Send + Sync {
    fn now(&self) -> Duration;
}

pub(super) trait EmergencyWriter: Send + Sync {
    fn write(&self, message: &str);
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FileMetadata {
    pub len: u64,
    pub is_file: bool,
    pub is_symlink: bool,
    pub modified: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SinkState {
    Healthy,
    Degraded,
    Recovering,
}

pub(super) struct FileSinkLifecycle {
    path: PathBuf,
    writer: Option<Box<dyn SinkWriter>>,
    state: SinkState,
    retry_at: Duration,
    max_bytes: u64,
    max_backups: usize,
    retention_days: u64,
    files: Arc<dyn FileOps>,
    clock: Arc<dyn MonotonicClock>,
    emergency: Arc<dyn EmergencyWriter>,
}

impl FileSinkLifecycle {
    pub fn start(
        path: PathBuf,
        max_bytes: u64,
        max_backups: usize,
        retention_days: u64,
        files: Arc<dyn FileOps>,
        clock: Arc<dyn MonotonicClock>,
        emergency: Arc<dyn EmergencyWriter>,
    ) -> Self {
        let mut sink = Self {
            path,
            writer: None,
            state: SinkState::Recovering,
            retry_at: Duration::ZERO,
            max_bytes: max_bytes.max(1),
            max_backups,
            retention_days,
            files,
            clock,
            emergency,
        };
        match sink.prepare_and_open() {
            Ok(()) => sink.state = SinkState::Healthy,
            Err(error) => sink.degrade("startup open", &error),
        }
        // Retention is an initialization action even when opening this sink degraded.
        sink.cleanup_retention();
        sink
    }

    #[cfg(test)]
    pub fn state(&self) -> SinkState {
        self.state
    }

    pub fn write_line(&mut self, line: &str) {
        if self.state != SinkState::Healthy && !self.try_recover() {
            self.fallback_record(line);
            return;
        }

        if let Err(error) = self.rotate_if_needed() {
            self.degrade("rotate", &error);
            self.fallback_record(line);
            return;
        }

        let result = self
            .writer
            .as_mut()
            .ok_or_else(|| io::Error::other("healthy sink has no writer"))
            .and_then(|writer| writer.write_all(line.as_bytes()))
            .and_then(|()| {
                self.writer
                    .as_mut()
                    .expect("writer checked above")
                    .write_all(b"\n")
            })
            .and_then(|()| self.writer.as_mut().expect("writer checked above").flush());
        if let Err(error) = result {
            self.degrade("write/flush", &error);
            self.fallback_record(line);
        }
    }

    pub fn flush(&mut self) {
        if self.state != SinkState::Healthy {
            return;
        }
        if let Some(writer) = self.writer.as_mut() {
            if let Err(error) = writer.flush() {
                self.degrade("explicit flush", &error);
            }
        }
    }

    fn prepare_and_open(&mut self) -> io::Result<()> {
        match self.files.metadata(&self.path) {
            Ok(metadata) if metadata.len >= self.max_bytes => self.rotate_active()?,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.writer = Some(self.files.open(&self.path)?);
        Ok(())
    }

    fn try_recover(&mut self) -> bool {
        if self.clock.now() < self.retry_at {
            return false;
        }
        self.state = SinkState::Recovering;
        match self.files.open(&self.path) {
            Ok(writer) => {
                self.writer = Some(writer);
                self.state = SinkState::Healthy;
                true
            }
            Err(error) => {
                self.degrade("recovery reopen", &error);
                false
            }
        }
    }

    fn rotate_if_needed(&mut self) -> io::Result<()> {
        let metadata = self.files.metadata(&self.path)?;
        if metadata.len < self.max_bytes {
            return Ok(());
        }
        if let Some(writer) = self.writer.as_mut() {
            writer.flush()?;
        }
        self.writer = None;
        self.rotate_active()?;
        self.writer = Some(self.files.open(&self.path)?);
        self.cleanup_retention();
        Ok(())
    }

    fn rotate_active(&self) -> io::Result<()> {
        if self.max_backups == 0 {
            return self.files.remove(&self.path);
        }
        for index in (1..=self.max_backups).rev() {
            let from = rotated_path(&self.path, index);
            // Existence is fallible by contract: never turn an error into `false`.
            if !self.files.exists(&from)? {
                continue;
            }
            if index == self.max_backups {
                self.files.remove(&from)?;
            } else {
                self.files
                    .rename(&from, &rotated_path(&self.path, index + 1))?;
            }
        }
        self.files.rename(&self.path, &rotated_path(&self.path, 1))
    }

    fn cleanup_retention(&self) {
        if self.retention_days == 0 {
            return;
        }
        let Some(parent) = self.path.parent() else {
            return;
        };
        let entries = match self.files.read_dir(parent) {
            Ok(entries) => entries,
            Err(error) => {
                self.report("retention read_dir", &error);
                return;
            }
        };
        let cutoff = Duration::from_secs(self.retention_days.saturating_mul(86_400));
        for candidate in entries {
            if !is_backup_of(&candidate, &self.path) {
                continue;
            }
            let metadata = match self.files.metadata(&candidate) {
                Ok(metadata) => metadata,
                Err(error) => {
                    self.report("retention metadata", &error);
                    continue;
                }
            };
            if !metadata.is_file || metadata.is_symlink {
                continue;
            }
            let old = SystemTime::now()
                .duration_since(metadata.modified)
                .is_ok_and(|age| age >= cutoff);
            if old {
                if let Err(error) = self.files.remove(&candidate) {
                    self.report("retention remove", &error);
                }
            }
        }
    }

    fn degrade(&mut self, operation: &str, error: &io::Error) {
        self.writer = None;
        self.state = SinkState::Degraded;
        self.retry_at = self.clock.now().saturating_add(RECOVERY_INTERVAL);
        self.report(operation, error);
    }

    fn report(&self, operation: &str, error: &io::Error) {
        self.emergency.write(&format!(
            "aemeath logging emergency: sink={} operation={operation} error={error}",
            self.path.display()
        ));
    }

    fn fallback_record(&self, line: &str) {
        self.emergency.write(line);
    }
}

pub(super) struct StdFileOps;

impl FileOps for StdFileOps {
    fn open(&self, path: &Path) -> io::Result<Box<dyn SinkWriter>> {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Box::new(std::io::BufWriter::new(file)))
    }

    fn metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        let link = fs::symlink_metadata(path)?;
        Ok(FileMetadata {
            len: link.len(),
            is_file: link.file_type().is_file(),
            is_symlink: link.file_type().is_symlink(),
            modified: link.modified()?,
        })
    }

    fn exists(&self, path: &Path) -> io::Result<bool> {
        path.try_exists()
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect()
    }
}

impl SinkWriter for std::io::BufWriter<fs::File> {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        Write::write_all(self, bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(self)
    }
}

pub(super) struct StdMonotonicClock {
    started: Instant,
}

impl Default for StdMonotonicClock {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl MonotonicClock for StdMonotonicClock {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
}

pub fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    path.with_file_name(format!("{file_name}.{index}"))
}

fn is_backup_of(candidate: &Path, active: &Path) -> bool {
    if candidate.parent() != active.parent() {
        return false;
    }
    let Some(candidate_name) = candidate.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(active_name) = active.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    candidate_name
        .strip_prefix(active_name)
        .and_then(|suffix| suffix.strip_prefix('.'))
        .is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub fn is_rotated_log_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some((base, suffix)) = file_name.rsplit_once('.') else {
        return false;
    };
    base.ends_with(".log") && !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn timestamp_rfc3339() -> String {
    let now: chrono::DateTime<chrono::Local> = chrono::Local::now();
    now.to_rfc3339()
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
