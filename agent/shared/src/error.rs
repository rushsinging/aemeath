//! Error handling utilities
//!
//! Provides unified error types and user-friendly error display.

use thiserror::Error;

/// Main error type for the application
#[derive(Debug, Error)]
pub enum AemeathError {
    /// API error
    #[error("API error: {message}")]
    Api {
        message: String,
        code: Option<String>,
    },

    /// Authentication error
    #[error("Authentication error: {message}")]
    Auth { message: String },

    /// Configuration error
    #[error("Configuration error: {message}")]
    Config { message: String },

    /// Tool execution error
    #[error("Tool '{tool}' error: {message}")]
    Tool { tool: String, message: String },

    /// File I/O error
    #[error("File error: {path}: {message}")]
    File { path: String, message: String },

    /// Network error
    #[error("Network error: {message}")]
    Network { message: String },

    /// Permission denied
    #[error("Permission denied: {message}")]
    Permission { message: String },

    /// Session error
    #[error("Session error: {message}")]
    Session { message: String },

    /// Invalid input
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },

    /// Rate limit exceeded
    #[error("Rate limit exceeded. Please wait {retry_after}s before retrying.")]
    RateLimit { retry_after: u64 },

    /// Token limit exceeded
    #[error("Token limit exceeded: used {used}, limit {limit}")]
    TokenLimit { used: u64, limit: u64 },

    /// Timeout
    #[error("Timeout after {seconds}s: {message}")]
    Timeout { seconds: u64, message: String },

    /// Internal error (should not be shown to user directly)
    #[error("Internal error: {message}")]
    Internal { message: String },

    /// Unknown error
    #[error("Unknown error: {message}")]
    Unknown { message: String },
}

/// Result type alias
pub type Result<T> = std::result::Result<T, AemeathError>;

/// Error display helper
pub struct ErrorDisplay<'a>(&'a AemeathError);

impl<'a> ErrorDisplay<'a> {
    /// Create a new error display helper
    pub fn new(error: &'a AemeathError) -> Self {
        Self(error)
    }

    /// Get user-friendly error message
    pub fn user_message(&self) -> String {
        match self.0 {
            AemeathError::Api { message, code } => {
                let code_part = code
                    .as_ref()
                    .map(|c| format!(" (code: {})", c))
                    .unwrap_or_default();
                format!("API 调用失败: {}{}", message, code_part)
            }
            AemeathError::Auth { message } => {
                format!("认证失败: {}. 请检查 API key 是否正确设置。", message)
            }
            AemeathError::Config { message } => {
                format!("配置错误: {}", message)
            }
            AemeathError::Tool { tool, message } => {
                format!("工具 '{}' 执行失败: {}", tool, message)
            }
            AemeathError::File { path, message } => {
                format!("文件 '{}' 操作失败: {}", path, message)
            }
            AemeathError::Network { message } => {
                format!("网络错误: {}. 请检查网络连接。", message)
            }
            AemeathError::Permission { message } => {
                format!("权限被拒绝: {}", message)
            }
            AemeathError::Session { message } => {
                format!("会话错误: {}", message)
            }
            AemeathError::InvalidInput { message } => {
                format!("输入无效: {}", message)
            }
            AemeathError::RateLimit { retry_after } => {
                format!("API 请求频率超限。请等待 {} 秒后重试。", retry_after)
            }
            AemeathError::TokenLimit { used, limit } => {
                format!(
                    "Token 超限: 已使用 {}, 限制 {}. 请精简输入或增加上下文大小。",
                    used, limit
                )
            }
            AemeathError::Timeout { seconds, message } => {
                format!("操作超时 ({}秒): {}", seconds, message)
            }
            AemeathError::Internal { .. } => "内部错误，请稍后重试或联系开发者。".to_string(),
            AemeathError::Unknown { message } => {
                format!("未知错误: {}", message)
            }
        }
    }

    /// Get error suggestions
    pub fn suggestions(&self) -> Vec<String> {
        match self.0 {
            AemeathError::Auth { .. } => vec![
                "检查 ANTHROPIC_API_KEY 环境变量是否设置".to_string(),
                "确认 API key 是否有效".to_string(),
                "尝试重新生成 API key".to_string(),
            ],
            AemeathError::Api { .. } => vec![
                "检查网络连接".to_string(),
                "确认 API base URL 是否正确".to_string(),
                "查看是否超过 API 配额".to_string(),
            ],
            AemeathError::RateLimit { .. } => vec![
                "等待一段时间后重试".to_string(),
                "减少并发请求数量".to_string(),
                "升级 API 配额".to_string(),
            ],
            AemeathError::TokenLimit { .. } => vec![
                "精简输入内容".to_string(),
                "使用更大的 context_size".to_string(),
                "清除部分对话历史".to_string(),
            ],
            AemeathError::Network { .. } => vec![
                "检查网络连接".to_string(),
                "确认防火墙设置".to_string(),
                "尝试使用代理".to_string(),
            ],
            AemeathError::File { .. } => vec![
                "确认文件路径是否正确".to_string(),
                "检查文件权限".to_string(),
                "确认磁盘空间是否充足".to_string(),
            ],
            AemeathError::Permission { .. } => vec![
                "检查是否需要用户授权".to_string(),
                "使用 --allow-all 参数跳过权限检查（慎用）".to_string(),
                "配置 auto_approve 工具列表".to_string(),
            ],
            _ => vec!["请检查输入参数是否正确".to_string()],
        }
    }

    /// Format full error message with suggestions
    pub fn full_message(&self) -> String {
        let mut output = format!("❌ {}\n", self.user_message());
        let suggestions = self.suggestions();
        if !suggestions.is_empty() {
            output.push_str("\n建议:\n");
            for (i, suggestion) in suggestions.iter().enumerate() {
                output.push_str(&format!("  {}. {}\n", i + 1, suggestion));
            }
        }
        output
    }
}

impl AemeathError {
    /// Create from std io error
    pub fn from_io(e: std::io::Error, path: impl Into<String>) -> Self {
        AemeathError::File {
            path: path.into(),
            message: e.to_string(),
        }
    }

    /// Create from serde json error
    pub fn from_serde_json(e: serde_json::Error) -> Self {
        AemeathError::InvalidInput {
            message: format!("JSON 解析错误: {}", e),
        }
    }

    /// Create an API error from HTTP status
    pub fn from_http_status(status: u16, message: String) -> Self {
        match status {
            401 => AemeathError::Auth {
                message: "API key 无效或已过期".to_string(),
            },
            403 => AemeathError::Permission {
                message: "无权限访问此 API".to_string(),
            },
            429 => AemeathError::RateLimit { retry_after: 60 },
            500..=599 => AemeathError::Api {
                message,
                code: Some(status.to_string()),
            },
            _ => AemeathError::Api {
                message,
                code: Some(status.to_string()),
            },
        }
    }

    /// Create a network error
    pub fn network(message: impl Into<String>) -> Self {
        AemeathError::Network {
            message: message.into(),
        }
    }

    /// Create a timeout error
    pub fn timeout(seconds: u64, message: impl Into<String>) -> Self {
        AemeathError::Timeout {
            seconds,
            message: message.into(),
        }
    }

    /// Get display helper
    pub fn display(&self) -> ErrorDisplay<'_> {
        ErrorDisplay::new(self)
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AemeathError::RateLimit { .. }
                | AemeathError::Network { .. }
                | AemeathError::Timeout { .. }
        )
    }

    /// Get retry after seconds (if applicable)
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            AemeathError::RateLimit { retry_after } => Some(*retry_after),
            _ => None,
        }
    }
}

/// Error context for tracking error source
#[derive(Debug)]
pub struct ErrorContext {
    /// Where the error occurred
    pub location: String,
    /// Additional context
    pub context: Option<String>,
    /// Timestamp
    pub timestamp: u64,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(location: impl Into<String>, timestamp: u64) -> Self {
        Self {
            location: location.into(),
            context: None,
            timestamp,
        }
    }

    /// Add context
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

/// Error with context
#[derive(Debug)]
pub struct ErrorWithContext {
    /// The error
    pub error: AemeathError,
    /// Context
    pub context: ErrorContext,
}

impl std::fmt::Display for ErrorWithContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)?;
        if let Some(ctx) = &self.context.context {
            write!(f, "\nContext: {}", ctx)?;
        }
        write!(f, "\nLocation: {}", self.context.location)?;
        Ok(())
    }
}

impl std::error::Error for ErrorWithContext {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod error_tests;
