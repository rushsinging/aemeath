//! SDK 错误类型。

use std::fmt;

/// SDK 层错误。
#[derive(Debug)]
pub enum SdkError {
    /// Runtime 初始化失败。
    Init(String),
    /// Chat 执行错误。
    Chat(String),
    /// Session 操作错误（通用，无法分类时使用）。
    Session(String),
    /// Session 不存在（`--resume <id>` 找不到文件）。
    /// 上层应提示「session 不存在」并以非零退出码退出，而非静默启动空 session。
    SessionNotFound { id: String },
    /// Session 文件损坏（JSON 解析失败且 .bak 回退失败）。
    /// `corrupt_path` 指向已被转存的 `.corrupt` 文件，供用户手工抢救。
    SessionCorrupt {
        id: String,
        parse_err: String,
        corrupt_path: String,
    },
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
            SdkError::SessionNotFound { id } => {
                write!(f, "Session 不存在: {id}")
            }
            SdkError::SessionCorrupt {
                id,
                parse_err,
                corrupt_path,
            } => {
                write!(
                    f,
                    "Session {id} 损坏（{parse_err}），原文件已转存到 {corrupt_path}"
                )
            }
            SdkError::Cancelled => write!(f, "操作已取消"),
            SdkError::Internal(msg) => write!(f, "内部错误: {msg}"),
        }
    }
}

impl std::error::Error for SdkError {}
