//! Session 恢复的共享逻辑。
//!
//! 提供 `SessionRestore`，把磁盘上的 `Session` 还原成运行时需要的活跃链状态：
//! 活跃消息、冻结段、compact 摘要、created_at 等。
//!
//! 设计目标（DRY）：消除 `trait_session::load_session_impl` 和
//! `loop_runner::PendingCommand::ResumeSession` 两条路径各自重复实现 resume
//! 还原逻辑的多路并存问题。两个调用点都应通过 `SessionRestore::from_session`
//! 获取活跃链相关数据。

use super::message_integrity::{check_message_integrity, deep_clean_messages, sanitize_messages};
use crate::session::{ChatChain, ChatSegment, SegmentKind, Session};
use share::message::Message;

/// 从 `Session` 还原出的活跃链运行时状态。
///
/// 字段都是「纯数据」——不包含 workspace 等 IO 副作用状态；调用方各自处理
/// workspace 恢复（trait_session 走 `WorkspacePersist::restore`，loop_runner
/// 由外部 set workspace）。
#[derive(Debug, Clone)]
pub struct SessionRestore {
    /// 经过 sanitize + deep_clean 修剪后的活跃链消息。
    pub active_messages: Vec<Message>,
    /// 按 user turn 分段的活跃链（从 `active_messages` 用 `from_flat_messages` 构造）。
    pub active_chain: ChatChain,
    /// 冻结的历史 chat 段（最后一个 Compact 段之前的全部段；若无 Compact 段则为空）。
    pub frozen_chats: Vec<ChatSegment>,
    /// 活跃链的 compact 摘要（活跃链首个 Compact 段的 summary），无则 None。
    pub active_summary: Option<String>,
    /// session 创建时间戳（ISO 8601 字符串，原样来自 `Session.created_at`）。
    pub created_at: String,
    /// 是否需要在 loop-top idle 门跳过首个 pending user turn（resume 场景恒为 true，见 #503）。
    pub skip_first_pending_turn: bool,
    /// sanitize 修剪掉的消息数。
    pub trimmed: usize,
    /// deep_clean 修复的问题数（仅在 message_integrity 检出问题时非 0）。
    pub repaired: usize,
}

impl SessionRestore {
    /// 从磁盘 `Session` 提取活跃链运行时状态。纯函数，无 IO 副作用。
    ///
    /// 算法：
    /// 1. 用 `ChatChain::from_chats` 构造活跃链视图。
    /// 2. frozen_chats = 活跃链起点之前的全部段。
    /// 3. 活跃消息经 sanitize + deep_clean 修剪。
    pub fn from_session(session: &Session) -> Self {
        let chain = ChatChain::from_chats(&session.chats);
        let summary = chain.active_summary().map(|s| s.to_string());

        // 活跃链起点 = 最后一个 Compact 段索引（与 ChatChain::from_chats 一致）。
        // frozen_chats = 起点之前的全部段；若起点是首个 Compact 段，则无 frozen。
        let active_start = session
            .chats
            .iter()
            .rposition(|s| s.kind == SegmentKind::Compact)
            .unwrap_or(0);
        let frozen_chats: Vec<ChatSegment> = session.chats[..active_start].to_vec();

        let mut messages = chain.messages();
        let trimmed = {
            let before = messages.len();
            sanitize_messages(&mut messages);
            before.saturating_sub(messages.len())
        };
        let repaired = {
            let integrity = check_message_integrity(&messages);
            if integrity.has_issues() {
                deep_clean_messages(&mut messages)
            } else {
                0
            }
        };

        Self {
            active_messages: messages,
            active_chain: chain,
            frozen_chats,
            active_summary: summary,
            created_at: session.created_at.clone(),
            skip_first_pending_turn: true,
            trimmed,
            repaired,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionMetadata;
    use share::message::Role;

    fn segment(parent: Option<String>, kind: SegmentKind, msgs: Vec<Message>) -> ChatSegment {
        ChatSegment {
            id: uuid::Uuid::new_v4().to_string(),
            parent_id: parent,
            kind,
            summary: None,
            messages: msgs,
        }
    }

    fn empty_session_with_chats(chats: Vec<ChatSegment>) -> Session {
        Session {
            id: "test-session".to_string(),
            cwd: "/tmp".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
            chats,
            messages: Vec::new(),
            metadata: SessionMetadata::default(),
            tasks: None,
            workspace: None,
        }
    }

    #[test]
    fn restores_messages_from_single_chat() {
        let chats = vec![segment(
            None,
            SegmentKind::Normal,
            vec![
                Message::user("hello"),
                Message::placeholder(Role::Assistant),
            ],
        )];
        let session = empty_session_with_chats(chats);

        let restore = SessionRestore::from_session(&session);

        assert_eq!(restore.active_messages.len(), 2);
        assert!(restore.frozen_chats.is_empty());
        assert!(restore.active_summary.is_none());
        assert_eq!(restore.created_at, "2025-01-01T00:00:00Z");
        assert!(restore.skip_first_pending_turn);
        // active_chain 从扁平消息重建：1 个真实 user turn → 1 segment
        assert_eq!(restore.active_chain.active_segments().len(), 1);
    }

    #[test]
    fn splits_frozen_and_active_at_compact() {
        let root = segment(
            None,
            SegmentKind::Normal,
            vec![Message::user("q1"), Message::placeholder(Role::Assistant)],
        );
        let root_id = root.id.clone();
        let compact = segment(
            Some(root_id.clone()),
            SegmentKind::Compact,
            vec![Message::user("c1"), Message::placeholder(Role::Assistant)],
        );
        let compact_id = compact.id.clone();
        let active = segment(
            Some(compact_id.clone()),
            SegmentKind::Normal,
            vec![Message::user("q2"), Message::placeholder(Role::Assistant)],
        );
        let session = empty_session_with_chats(vec![root, compact, active]);

        let restore = SessionRestore::from_session(&session);

        // 活跃链：从 compact 起算（含 compact 段自身 + active 段）→ 4 条
        assert_eq!(
            restore.active_messages.len(),
            4,
            "active should include compact + active segment messages"
        );
        // 冻结段：compact 之前的所有段
        assert_eq!(restore.frozen_chats.len(), 1);
        assert_eq!(restore.frozen_chats[0].id, root_id);
    }

    #[test]
    fn ignores_legacy_empty_messages_field() {
        // 模拟 PR #643 之后的存盘：messages 字段为空，所有消息在 chats 里
        let mut session = empty_session_with_chats(vec![segment(
            None,
            SegmentKind::Normal,
            vec![
                Message::user("only_in_chats"),
                Message::placeholder(Role::Assistant),
            ],
        )]);
        session.messages = Vec::new();

        let restore = SessionRestore::from_session(&session);

        assert_eq!(restore.active_messages.len(), 2);
    }

    #[test]
    fn active_chain_from_legacy_single_segment_is_single_segment() {
        // 旧 session：3 个 user turn 被存在单个 Normal segment 中。
        // from_flat_messages 不猜测边界，恢复为单段。
        // 运行时后续 start_new_segment() 会在新 user turn 时创建正确边界。
        let chats = vec![segment(
            None,
            SegmentKind::Normal,
            vec![
                Message::user("turn1"),
                Message::placeholder(Role::Assistant),
                Message::user("turn2"),
                Message::placeholder(Role::Assistant),
                Message::user("turn3"),
                Message::placeholder(Role::Assistant),
            ],
        )];
        let session = empty_session_with_chats(chats);

        let restore = SessionRestore::from_session(&session);

        assert_eq!(restore.active_messages.len(), 6);
        // from_flat_messages 不切分段——单段保留
        assert_eq!(restore.active_chain.active_segments().len(), 1);
        assert_eq!(restore.active_chain.messages().len(), 6);
    }
}
