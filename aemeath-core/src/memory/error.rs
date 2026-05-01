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
mod tests {
    use super::*;

    #[test]
    fn test_memory_error_file() {
        let err = MemoryError::file("/tmp/memory.json", std::io::Error::other("denied"));

        assert!(matches!(err, MemoryError::File { .. }));
        assert!(err.to_string().contains("/tmp/memory.json"));
        assert!(err.to_string().contains("denied"));
    }

    #[test]
    fn test_memory_error_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let err = MemoryError::json(json_err);

        assert!(matches!(err, MemoryError::Json { .. }));
        assert!(err.to_string().contains("记忆 JSON 解析失败"));
    }

    #[test]
    fn test_memory_error_not_found() {
        let err = MemoryError::not_found("mem-1");

        assert!(matches!(err, MemoryError::NotFound { .. }));
        assert_eq!(err.to_string(), "记忆不存在: mem-1");
    }
}
