use async_trait::async_trait;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultBlobRef {
    locator: String,
}

impl ToolResultBlobRef {
    pub fn new(locator: String) -> Self {
        Self { locator }
    }

    pub fn locator(&self) -> &str {
        &self.locator
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultBlobError {
    message: String,
}

impl ToolResultBlobError {
    pub fn write(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn invalid_key(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ToolResultBlobError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolResultBlobError {}

#[async_trait]
pub trait ToolResultBlobPort: Send + Sync {
    async fn write_once(
        &self,
        session_id: &str,
        tool_use_id: &str,
        bytes: &[u8],
    ) -> Result<ToolResultBlobRef, ToolResultBlobError>;
}
