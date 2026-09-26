//! Run 目的：区分会话对话 Run 与只执行上下文压缩的 Run。
//!
//! 目的是 Run 聚合的规格事实（`RunSpec` 持有），决定哪些迁移合法：
//! `ManualCompaction` 的 Run 只承担一次手动上下文压缩，压缩完成后经
//! `CompactionOnlySettled` 回到输入排空阶段收口，**NEVER** 进入模型调用。

/// 单次 Run 的目的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunIntent {
    /// 会话对话（默认）：按用户输入驱动 Step 与模型调用。
    Conversation,
    /// 只执行一次手动上下文压缩，不调用模型。
    ManualCompaction,
}
