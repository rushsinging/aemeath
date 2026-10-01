use thiserror::Error;

/// Memory system result type.
pub type MemoryResult<T> = std::result::Result<T, MemoryError>;

/// Memory system errors.
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("记忆文件操作失败: {path}: {message}")]
    File { path: String, message: String },

    #[error("记忆 JSON 解析失败: {message}")]
    Json { message: String },

    #[error("记忆不存在: {id}")]
    NotFound { id: String },

    #[error("记忆配置无效: {message}")]
    Config { message: String },

    #[error("记忆输入无效: {message}")]
    InvalidInput { message: String },
}

impl MemoryError {
    pub fn file(path: impl Into<String>, error: std::io::Error) -> Self {
        Self::File {
            path: path.into(),
            message: error.to_string(),
        }
    }

    pub fn json(error: serde_json::Error) -> Self {
        Self::Json {
            message: error.to_string(),
        }
    }

    pub fn not_found(id: impl Into<String>) -> Self {
        Self::NotFound { id: id.into() }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
