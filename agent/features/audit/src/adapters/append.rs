use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sdk::SessionId;
use storage::{SafeOpenOptions, SafePathSegment, SafeStorageFileType, SafeStorageRoot};

use crate::{
    AppendLogError, AppendLogLine, AppendLogNamespace, AppendLogReader, AppendLogStream,
    UsageAppendStorePort,
};

const USAGE_NAMESPACE: &str = "usage";
const JSONL_SUFFIX: &str = ".jsonl";

type StreamLock = Arc<Mutex<()>>;

pub fn file_usage_append_store(root: SafeStorageRoot) -> FileUsageAppendStore {
    FileUsageAppendStore::new(root)
}

pub struct FileUsageAppendStore {
    root: SafeStorageRoot,
    stream_locks: Mutex<HashMap<String, StreamLock>>,
}

impl FileUsageAppendStore {
    pub fn new(root: SafeStorageRoot) -> Self {
        Self {
            root,
            stream_locks: Mutex::new(HashMap::new()),
        }
    }

    pub fn stream_for_session(&self, session_id: &SessionId) -> AppendLogStream {
        AppendLogStream::for_session(session_id)
    }

    fn stream_lock(&self, stream: &AppendLogStream) -> Result<StreamLock, AppendLogError> {
        let mut locks = self
            .stream_locks
            .lock()
            .map_err(|_| AppendLogError::Closed)?;
        Ok(locks
            .entry(stream.as_str().to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }

    fn usage_dir(&self) -> Result<storage::SafeStorageDir, AppendLogError> {
        let namespace = SafePathSegment::from_str(USAGE_NAMESPACE)
            .map_err(|_| AppendLogError::InvalidNamespace)?;
        self.root
            .ensure_dir(&[namespace])
            .map_err(|_| AppendLogError::Io)
    }

    fn stream_file_name(stream: &AppendLogStream) -> Result<SafePathSegment, AppendLogError> {
        SafePathSegment::from_str(&format!("{}{JSONL_SUFFIX}", stream.as_str()))
            .map_err(|_| AppendLogError::InvalidStream)
    }

    fn map_open_error(error: storage::StorageError) -> AppendLogError {
        match error.kind() {
            storage::StorageErrorKind::InvalidKey => AppendLogError::InvalidStream,
            _ => AppendLogError::Io,
        }
    }

    fn validate_payload(bytes: &[u8]) -> Result<(), AppendLogError> {
        if bytes.is_empty()
            || bytes.last() != Some(&b'\n')
            || bytes[..bytes.len() - 1].contains(&b'\n')
        {
            return Err(AppendLogError::InvalidPayload);
        }
        Ok(())
    }

    fn append_sync(&self, stream: &AppendLogStream, bytes: &[u8]) -> Result<(), AppendLogError> {
        Self::validate_payload(bytes)?;
        let lock = self.stream_lock(stream)?;
        let _guard = lock.lock().map_err(|_| AppendLogError::Closed)?;
        let dir = self.usage_dir()?;
        let name = Self::stream_file_name(stream)?;
        let mut file = dir
            .create_or_open(
                &name,
                SafeOpenOptions {
                    read: true,
                    append: true,
                },
            )
            .map_err(Self::map_open_error)?;
        file.write_all(bytes).map_err(|_| AppendLogError::Io)
    }

    fn flush_sync(&self, stream: &AppendLogStream) -> Result<(), AppendLogError> {
        let lock = self.stream_lock(stream)?;
        let _guard = lock.lock().map_err(|_| AppendLogError::Closed)?;
        let dir = self.usage_dir()?;
        let name = Self::stream_file_name(stream)?;
        let file = dir
            .open_existing(
                &name,
                SafeOpenOptions {
                    read: true,
                    append: true,
                },
            )
            .map_err(Self::map_open_error)?;
        file.sync_data().map_err(|_| AppendLogError::Io)
    }

    fn read_sync(&self, stream: &AppendLogStream) -> Result<AppendLogReader, AppendLogError> {
        let lock = self.stream_lock(stream)?;
        let _guard = lock.lock().map_err(|_| AppendLogError::Closed)?;
        let dir = self.usage_dir()?;
        let name = Self::stream_file_name(stream)?;
        let file = dir
            .open_existing(
                &name,
                SafeOpenOptions {
                    read: true,
                    append: false,
                },
            )
            .map_err(Self::map_open_error)?;
        let mut reader = BufReader::new(file);
        let mut lines = Vec::new();
        loop {
            let mut line = Vec::new();
            let count = reader
                .read_until(b'\n', &mut line)
                .map_err(|_| AppendLogError::Io)?;
            if count == 0 {
                break;
            }
            let terminated = line.last() == Some(&b'\n');
            if terminated {
                line.pop();
            }
            lines.push(AppendLogLine::new(line, terminated));
        }
        Ok(AppendLogReader::new(lines))
    }

    fn list_sync(
        &self,
        namespace: &AppendLogNamespace,
    ) -> Result<Vec<AppendLogStream>, AppendLogError> {
        if namespace.as_str() != USAGE_NAMESPACE {
            return Err(AppendLogError::InvalidNamespace);
        }
        let dir = self.usage_dir()?;
        let mut streams = Vec::new();
        for entry in dir.entries().map_err(|_| AppendLogError::Io)? {
            if entry.file_type() != SafeStorageFileType::RegularFile {
                continue;
            }
            let Some(stream) = entry.name().as_str().strip_suffix(JSONL_SUFFIX) else {
                continue;
            };
            if SafePathSegment::from_str(stream).is_ok() {
                streams.push(AppendLogStream::new(stream.to_string()));
            }
        }
        streams.sort();
        Ok(streams)
    }
}

#[async_trait]
impl UsageAppendStorePort for FileUsageAppendStore {
    async fn append(&self, stream: &AppendLogStream, bytes: &[u8]) -> Result<(), AppendLogError> {
        self.append_sync(stream, bytes)
    }

    async fn flush(&self, stream: &AppendLogStream) -> Result<(), AppendLogError> {
        self.flush_sync(stream)
    }

    async fn read(&self, stream: &AppendLogStream) -> Result<AppendLogReader, AppendLogError> {
        self.read_sync(stream)
    }

    async fn list_streams(
        &self,
        namespace: &AppendLogNamespace,
    ) -> Result<Vec<AppendLogStream>, AppendLogError> {
        self.list_sync(namespace)
    }
}
