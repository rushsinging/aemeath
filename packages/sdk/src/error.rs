//! SDK 错误类型。

use std::fmt;

/// SDK 层错误。
#[derive(Debug)]
pub enum SdkError {
    /// Runtime 初始化失败。
    Init(String),
    /// Chat 执行错误。
    Chat(String),
    /// Session 操作错误。
    Session(String),
    /// 操作被取消。
    Cancelled,
    /// 内部错误。
    Internal(String),
}

impl fmt::Display for SdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdkError::Init(msg) => write!(f, "初始化失败: {msg}"),
            SdkError::Chat(msg) => write!(f, "Chat 错误: {msg}"),
            SdkError::Session(msg) => write!(f, "Session 错误: {msg}"),
            SdkError::Cancelled => write!(f, "操作已取消"),
            SdkError::Internal(msg) => write!(f, "内部错误: {msg}"),
        }
    }
}

impl std::error::Error for SdkError {}
