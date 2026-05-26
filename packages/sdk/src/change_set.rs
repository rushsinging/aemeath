//! 变更标记 — 快照 + 变更通道模式。
//!
//! Runtime 侧推送变更标记，CLI 侧按标记拉取最新快照。

bitflags::bitflags! {
    /// 标记哪些领域发生了变更。
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ChangeSet: u8 {
        const SESSION = 0b0001;
        const COST    = 0b0010;
        const TASKS   = 0b0100;
        const PROJECT = 0b1000;
    }
}
