//! Context 域错误细分码词表（`DomainError::code` 的稳定取值）。
//!
//! 原 5 个边界错误枚举（SessionManagementError/AcceptedInputError/
//! ContextAppendError/ContextPortError/ToolReceiptMutationError）折叠为
//! `share::error::DomainError` 后，变体粒度由本词表的细分码承载：
//! 消费方（runtime resume 失败分类转译等）按 `code()` 匹配，常量即契约。

/// Session 不存在。
pub const SESSION_NOT_FOUND: &str = "session_not_found";
/// Session 不属于当前项目。
pub const SESSION_PROJECT_MISMATCH: &str = "session_project_mismatch";
/// Session 数据损坏。
pub const SESSION_CORRUPT: &str = "session_corrupt";
/// Session schema 版本过新。
pub const SESSION_UNSUPPORTED_VERSION: &str = "session_unsupported_version";
/// Session 底层存储失败。
pub const SESSION_STORAGE: &str = "session_storage";
/// Session 恢复失败。
pub const SESSION_RESUME: &str = "session_resume";
/// Session backing 读取失败。
pub const SESSION_REPOSITORY: &str = "session_repository";

/// RunStep 已接受输入冲突。
pub const ACCEPTED_INPUT_CONFLICT: &str = "accepted_input_conflict";
/// 已接受输入持久化失败。
pub const ACCEPTED_INPUT_STORAGE: &str = "accepted_input_storage";

/// Session revision 冲突。
pub const REVISION_CONFLICT: &str = "revision_conflict";
/// RunStep 内容冲突。
pub const CONTENT_CONFLICT: &str = "content_conflict";
/// Context 持久化失败。
pub const CONTEXT_APPEND_STORAGE: &str = "context_append_storage";

/// Prompt 物化失败。
pub const PROMPT_MATERIALIZATION: &str = "prompt_materialization";
/// Memory 物化失败。
pub const MEMORY_MATERIALIZATION: &str = "memory_materialization";
/// Context 压缩失败。
pub const COMPACT: &str = "compact";

/// Tool receipt 持久化失败。
pub const TOOL_RECEIPT_STORAGE: &str = "tool_receipt_storage";
/// Tool receipt identity 不匹配。
pub const IDENTITY_MISMATCH: &str = "identity_mismatch";
/// Tool receipt 状态转换非法。
pub const INVALID_TRANSITION: &str = "invalid_transition";
/// Tool receipt 已终态禁止覆盖。
pub const TERMINAL_STATE_CONFLICT: &str = "terminal_state_conflict";
