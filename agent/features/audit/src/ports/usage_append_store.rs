use std::fmt;

use async_trait::async_trait;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AppendLogNamespace(String);

impl AppendLogNamespace {
    pub fn usage() -> Self {
        Self("usage".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AppendLogStream(String);

impl AppendLogStream {
    pub fn for_session(session_id: &sdk::SessionId) -> Self {
        Self(session_id.as_str().to_string())
    }

    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendLogLine {
    bytes: Vec<u8>,
    terminated: bool,
}

impl AppendLogLine {
    pub(crate) fn new(bytes: Vec<u8>, terminated: bool) -> Self {
        Self { bytes, terminated }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn is_terminated(&self) -> bool {
        self.terminated
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendLogReader {
    lines: Vec<AppendLogLine>,
}

impl AppendLogReader {
    pub(crate) fn new(lines: Vec<AppendLogLine>) -> Self {
        Self { lines }
    }

    pub fn lines(&self) -> &[AppendLogLine] {
        &self.lines
    }

    pub fn into_lines(self) -> Vec<AppendLogLine> {
        self.lines
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppendLogError {
    Io,
    InvalidNamespace,
    InvalidStream,
    InvalidPayload,
    Closed,
}

impl fmt::Display for AppendLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Io => "追加日志 I/O 失败",
            Self::InvalidNamespace => "追加日志命名空间无效",
            Self::InvalidStream => "追加日志流无效",
            Self::InvalidPayload => "追加日志负载必须是单个换行终止记录",
            Self::Closed => "追加日志已关闭",
        })
    }
}

impl std::error::Error for AppendLogError {}

#[async_trait]
pub trait UsageAppendStorePort: Send + Sync {
    async fn append(&self, stream: &AppendLogStream, bytes: &[u8]) -> Result<(), AppendLogError>;
    async fn flush(&self, stream: &AppendLogStream) -> Result<(), AppendLogError>;
    async fn read(&self, stream: &AppendLogStream) -> Result<AppendLogReader, AppendLogError>;
    async fn list_streams(
        &self,
        namespace: &AppendLogNamespace,
    ) -> Result<Vec<AppendLogStream>, AppendLogError>;
}
