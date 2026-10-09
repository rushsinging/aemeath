//! Run 目的：区分会话对话 Run、只执行上下文压缩的 Run 与只执行 Memory 反思的 Run。
//!
//! 目的是 Run 聚合的规格事实（`RunSpec` 持有），决定哪些迁移合法：
//! `ManualCompaction` 的 Run 只承担一次手动上下文压缩——runtime 受理命令后由
//! `Run::begin_manual_compaction()` 置为 `Compacting`，压缩完成由状态机按“无活动
//! Step”回到输入排空阶段收口（reason `ManualCompactionSettled`），**NEVER** 进入模型调用。
//! `ManualReflection` 的 Run 只承担一次 Memory 反思——由 `Run::begin_manual_reflection()`
//! 置为 `Reflecting`，反思完成回到输入排空阶段收口（reason `ManualReflectionSettled`），
//! **NEVER** 进入模型调用、**NEVER** 走 ContextPort::build_window。

/// 单次 Run 的目的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunIntent {
    /// 会话对话（默认）：按用户输入驱动 Step 与模型调用。
    Conversation,
    /// 只执行一次手动上下文压缩，不调用模型。
    ManualCompaction,
    /// 只执行一次 Memory 反思，不调用模型。
    ManualReflection,
    /// 后台进程完成唤醒的对话 Run（#252）：行为与 `Conversation` 同构
    /// （模型调用、工具执行、正常取消），仅启动触发源不同——由
    /// WakeupMailbox 在无 active Run 时驱动。完成事实经
    /// `background_process` reminder 注入，intent 本体不携带任务数据。
    BackgroundProcessWakeup,
}
