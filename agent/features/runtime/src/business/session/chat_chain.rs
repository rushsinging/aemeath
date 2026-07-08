//! Chat 链结构：Session 内按 user 消息分段，compact 产生新链。
//!
//! ## 链语义
//!
//! ```text
//! 正常对话: [Normal(A,null)] → [Normal(B,A)] → [Normal(C,B)]
//! compact 后: [Normal(A,null)] → [Normal(B,A)] → [Normal(C,B)]   ← 旧链冻结
//!                                                                ↘
//!            [Compact(D,null, summary)] → [Normal(E,D)] → [Normal(F,E)]  ← 新链
//! ```
//!
//! resume 只加载活跃链（最后一个 `Compact` 段到末端），天然跳过被压缩的旧历史。

use sdk::ids::ChatId;
use serde::{Deserialize, Serialize};
use share::message::Message;

/// 段类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    /// 正常对话段（一条 user 消息 + 其触发的完整回合，含追问/多轮 tool）
    #[default]
    Normal,
    /// compact 产生的新链起点（`parent_id` 为 None）
    Compact,
}

/// Session 内的一个 chat 段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSegment {
    /// 段 ID（UUIDv7）
    pub id: String,
    /// 父段 ID；Normal 段指向前一段，Compact 段为 None（新链起点）
    #[serde(default)]
    pub parent_id: Option<String>,
    /// 段类型
    #[serde(default)]
    pub kind: SegmentKind,
    /// Compact 段的摘要文本（走 system 通道）；Normal 段为 None
    #[serde(default)]
    pub summary: Option<String>,
    /// 该段的消息列表
    #[serde(default)]
    pub messages: Vec<Message>,
}

impl ChatSegment {
    /// 创建 Normal 段
    pub fn normal(parent_id: Option<String>) -> Self {
        Self {
            id: ChatId::new_v7().to_string(),
            parent_id,
            kind: SegmentKind::Normal,
            summary: None,
            messages: Vec::new(),
        }
    }

    /// 创建 Compact 段（新链起点）
    pub fn compact(summary: String, recent_messages: Vec<Message>) -> Self {
        Self {
            id: ChatId::new_v7().to_string(),
            parent_id: None,
            kind: SegmentKind::Compact,
            summary: Some(summary),
            messages: recent_messages,
        }
    }
}

/// 运行时活跃链管理器。
///
/// 持有活跃链（最后一个 `Compact` 段或首个 `parent=None` 段到末端）的所有 segment，
/// 提供扁平 `messages()` 视图供 chat loop 使用，并追踪 compact summary。
#[derive(Debug, Clone, Default)]
pub struct ChatChain {
    /// 活跃链的所有段
    segments: Vec<ChatSegment>,
}

impl ChatChain {
    /// 从 Session 的全部 chats 中提取活跃链。
    ///
    /// 找到最后一个 `Compact` 段（无则首个 `parent_id == None` 段），向后取全部。
    pub fn from_chats(chats: &[ChatSegment]) -> Self {
        let start = chats
            .iter()
            .rposition(|s| s.kind == SegmentKind::Compact)
            .or_else(|| chats.iter().position(|s| s.parent_id.is_none()));
        let segments = match start {
            Some(idx) => chats[idx..].to_vec(),
            None => Vec::new(),
        };
        Self { segments }
    }

    /// 从已有段列表构造（测试 / restore 用）。
    pub fn from_segments(segments: Vec<ChatSegment>) -> Self {
        Self { segments }
    }

    /// 扁平视图：合并所有段的 messages（供 chat loop 使用）。
    pub fn messages(&self) -> Vec<Message> {
        self.segments
            .iter()
            .flat_map(|s| s.messages.iter().cloned())
            .collect()
    }

    /// `messages` 的语义别名——强调「派生读模型，用完即弃」。
    pub fn messages_flat(&self) -> Vec<Message> {
        self.messages()
    }

    /// 活跃链的 summary（首个 Compact 段的 summary）
    pub fn active_summary(&self) -> Option<&str> {
        let first = self.segments.first()?;
        if first.kind == SegmentKind::Compact {
            first.summary.as_deref()
        } else {
            None
        }
    }

    /// 追加消息到指定 segment。
    ///
    /// 若 segment 不存在则创建（parent_id 指向当前最后一个段）。
    /// segment ID 由 loop 在 turn 开始时生成，整个 turn 内复用。
    pub fn push(&mut self, msg: Message, segment_id: &str) {
        // 找到对应 segment
        let idx = self.segments.iter().position(|s| s.id == segment_id);
        match idx {
            Some(i) => self.segments[i].messages.push(msg),
            None => {
                // segment 不存在，创建并放入
                let parent_id = self.segments.last().map(|s| s.id.clone());
                let mut seg = ChatSegment {
                    id: segment_id.to_string(),
                    parent_id,
                    kind: SegmentKind::Normal,
                    summary: None,
                    messages: vec![msg],
                };
                let _ = &mut seg; // suppress unused warning
                self.segments.push(seg);
            }
        }
    }

    /// 整体替换活跃链（compact / resume 时使用）。
    pub fn replace_chain(&mut self, new_chain: ChatChain) {
        self.segments = new_chain.segments;
    }

    /// 从扁平消息列表构造链（单段）。
    ///
    /// **不猜测 turn 边界**——segment 边界只由 loop 在 turn 开始时生成 segment ID 控制。
    pub fn from_flat_messages(messages: Vec<Message>) -> Self {
        if messages.is_empty() {
            return Self::default();
        }
        let mut seg = ChatSegment::normal(None);
        seg.messages = messages;
        Self {
            segments: vec![seg],
        }
    }

    /// compact 分叉：用 summary + recent tail 替换活跃链。
    ///
    /// 旧链由调用方在持久化时保留（追加到 Session.chats），此处只持有新链。
    pub fn compact(&mut self, summary: String, recent_messages: Vec<Message>) {
        self.segments = vec![ChatSegment::compact(summary, recent_messages)];
    }

    /// 活跃链的段列表（供持久化 / 只读访问）
    pub fn active_segments(&self) -> &[ChatSegment] {
        &self.segments
    }

    /// 活跃链的段列表（可变访问，供 microcompact 等原地修改）
    pub fn active_segments_mut(&mut self) -> &mut [ChatSegment] {
        &mut self.segments
    }

    /// 所有段是否均为空消息
    pub fn is_empty(&self) -> bool {
        self.segments.iter().all(|s| s.messages.is_empty())
    }

    /// 扁平消息总数。
    pub fn message_count(&self) -> usize {
        self.segments.iter().map(|s| s.messages.len()).sum()
    }

    /// 最后一条消息的引用（跨段查找，跳过空段）。
    pub fn last_message(&self) -> Option<&Message> {
        self.segments
            .iter()
            .rev()
            .find_map(|seg| seg.messages.last())
    }

    /// 删除最后一条消息（跨 segment，undo 用）。
    ///
    /// 从末尾 segment 开始，跳过空 segment（移除之），直到弹出一条消息。
    pub fn pop_last_message(&mut self) -> Option<Message> {
        loop {
            let last = self.segments.last_mut()?;
            if last.messages.is_empty() {
                // 空段移除，继续找前一段
                self.segments.pop();
                continue;
            }
            return last.messages.pop();
        }
    }

    /// 清空所有段的所有消息（/clear 用）。
    pub fn clear(&mut self) {
        for seg in &mut self.segments {
            seg.messages.clear();
        }
    }

    /// 按扁平消息截断到指定长度（回滚用）。
    ///
    /// 从末尾逐条删除直到扁平总长度 == `len`。
    pub fn truncate_flat(&mut self, len: usize) {
        loop {
            let current = self.message_count();
            if current <= len {
                break;
            }
            // 从最后一个 segment 的末尾删除一条
            if let Some(last_seg) = self.segments.last_mut() {
                if !last_seg.messages.is_empty() {
                    last_seg.messages.pop();
                } else {
                    // 空 segment，移除
                    self.segments.pop();
                }
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::message::{ContentBlock, Message, Role};

    fn user_msg(text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            metadata: None,
        }
    }

    fn asst_msg(text: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            metadata: None,
        }
    }

    fn tool_result_msg() -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::Value::String("result".to_string()),
                is_error: false,
                text: Some("result".to_string()),
            }],
            metadata: None,
        }
    }

    #[test]
    fn test_segment_kind_default_is_normal() {
        assert_eq!(SegmentKind::default(), SegmentKind::Normal);
    }

    #[test]
    fn test_chat_segment_normal_has_no_summary() {
        let seg = ChatSegment::normal(None);
        assert_eq!(seg.kind, SegmentKind::Normal);
        assert!(seg.parent_id.is_none());
        assert!(seg.summary.is_none());
        assert!(seg.messages.is_empty());
    }

    #[test]
    fn test_chat_segment_compact_carries_summary_and_messages() {
        let msgs = vec![user_msg("recent")];
        let seg = ChatSegment::compact("summary text".to_string(), msgs);
        assert_eq!(seg.kind, SegmentKind::Compact);
        assert!(seg.parent_id.is_none());
        assert_eq!(seg.summary.as_deref(), Some("summary text"));
        assert_eq!(seg.messages.len(), 1);
        assert_eq!(seg.messages[0].text_content(), "recent");
    }

    #[test]
    fn test_from_chats_picks_last_compact_as_start() {
        // chats: [Normal(A,null), Normal(B,A), Compact(C,null), Normal(D,C)]
        let a = ChatSegment::normal(None);
        let a_id = a.id.clone();
        let mut b = ChatSegment::normal(Some(a_id.clone()));
        b.messages = vec![user_msg("b")];
        let mut compact_seg = ChatSegment::compact("sum".to_string(), vec![]);
        compact_seg.messages = vec![asst_msg("tail")];
        let compact_id = compact_seg.id.clone();
        let mut d = ChatSegment::normal(Some(compact_id));
        d.messages = vec![user_msg("d")];
        let chats = vec![a, b, compact_seg, d];

        let chain = ChatChain::from_chats(&chats);
        // 活跃链应从 Compact 段开始，包含 compact + d
        assert_eq!(chain.active_segments().len(), 2);
        assert_eq!(chain.active_segments()[0].kind, SegmentKind::Compact);
        assert_eq!(chain.active_summary(), Some("sum"));
        // 扁平 messages = compact 的 tail + d 的 messages
        let msgs = chain.messages();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].text_content(), "d");
    }

    #[test]
    fn test_from_chats_no_compact_uses_first_parent_none() {
        // 无 Compact 段时，从首个 parent=None 段开始
        let mut a = ChatSegment::normal(None);
        a.messages = vec![user_msg("a")];
        let mut b = ChatSegment::normal(Some(a.id.clone()));
        b.messages = vec![user_msg("b")];
        let chats = vec![a, b];

        let chain = ChatChain::from_chats(&chats);
        assert_eq!(chain.active_segments().len(), 2);
        assert!(chain.active_summary().is_none());
        assert_eq!(chain.messages().len(), 2);
    }

    #[test]
    fn test_from_chats_empty() {
        let chain = ChatChain::from_chats(&[]);
        assert!(chain.is_empty());
        assert_eq!(chain.messages().len(), 0);
    }

    #[test]
    fn test_push_appends_to_last_segment() {
        let mut chain = ChatChain {
            segments: vec![ChatSegment::normal(None)],
        };
        chain.push(user_msg("hello"), "seg1");
        assert_eq!(chain.messages().len(), 1);
        assert_eq!(chain.messages()[0].text_content(), "hello");
    }

    #[test]
    fn test_push_creates_segment_if_not_exists() {
        let mut chain = ChatChain {
            segments: vec![ChatSegment::normal(None)],
        };
        chain.push(user_msg("hello"), "new-seg");
        assert_eq!(chain.segments.len(), 2);
        assert_eq!(chain.segments[1].id, "new-seg");
        assert_eq!(chain.segments[1].messages.len(), 1);
        assert_eq!(chain.segments[1].messages[0].text_content(), "hello");
    }

    #[test]
    fn test_compact_replaces_chain_with_single_compact_segment() {
        let mut chain = ChatChain {
            segments: vec![ChatSegment::normal(None), ChatSegment::normal(None)],
        };
        let tail = vec![user_msg("tail")];
        chain.compact("summary".to_string(), tail);
        assert_eq!(chain.segments.len(), 1);
        assert_eq!(chain.segments[0].kind, SegmentKind::Compact);
        assert_eq!(chain.active_summary(), Some("summary"));
        assert_eq!(chain.messages().len(), 1);
        assert_eq!(chain.messages()[0].text_content(), "tail");
    }

    #[test]
    fn test_serde_roundtrip_normal_segment() {
        let seg = ChatSegment::normal(Some("parent-123".to_string()));
        let json = serde_json::to_string(&seg).unwrap();
        let de: ChatSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(de.kind, SegmentKind::Normal);
        assert_eq!(de.parent_id.as_deref(), Some("parent-123"));
    }

    #[test]
    fn test_serde_roundtrip_compact_segment() {
        let seg = ChatSegment::compact("summary text".to_string(), vec![user_msg("m")]);
        let json = serde_json::to_string(&seg).unwrap();
        let de: ChatSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(de.kind, SegmentKind::Compact);
        assert_eq!(de.summary.as_deref(), Some("summary text"));
        assert_eq!(de.messages.len(), 1);
    }

    #[test]
    fn test_serde_default_missing_fields() {
        // 旧格式 JSON 缺少 parent_id/kind/summary/messages 应能反序列化
        let json = format!("{{\"id\":\"{}\"}}", ChatId::new_v7());
        let de: ChatSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(de.kind, SegmentKind::Normal);
        assert!(de.parent_id.is_none());
        assert!(de.summary.is_none());
        assert!(de.messages.is_empty());
    }

    // ── 新增 API 测试 ──────────────────────────────────

    #[test]
    fn test_push_to_existing_segment_appends() {
        let mut chain = ChatChain {
            segments: vec![ChatSegment::normal(None)],
        };
        let seg_id = chain.segments[0].id.clone();
        chain.push(user_msg("hello"), &seg_id);
        assert_eq!(chain.messages().len(), 1);
        assert_eq!(chain.messages()[0].text_content(), "hello");
    }

    #[test]
    fn test_push_new_segment_creates_if_empty() {
        let mut chain = ChatChain::default();
        chain.push(user_msg("auto-seg"), "seg1");
        assert_eq!(chain.active_segments().len(), 1);
        assert_eq!(chain.messages().len(), 1);
    }

    #[test]
    fn test_messages_flat_equals_messages() {
        let mut chain = ChatChain {
            segments: vec![ChatSegment::normal(None)],
        };
        chain.push(user_msg("a"), "seg1");
        chain.push(asst_msg("b"), "seg1");
        // 逐元素比对（Message 无 PartialEq）
        let flat = chain.messages_flat();
        let msgs = chain.messages();
        assert_eq!(flat.len(), msgs.len());
        assert_eq!(flat[0].text_content(), msgs[0].text_content());
        assert_eq!(flat[1].text_content(), msgs[1].text_content());
    }

    #[test]
    fn test_replace_chain() {
        let mut chain = ChatChain {
            segments: vec![ChatSegment::normal(None)],
        };
        chain.push(user_msg("old"), "seg1");

        let mut new_chain = ChatChain::default();
        new_chain.push(user_msg("new1"), "seg2");
        new_chain.push(asst_msg("new2"), "seg2");

        chain.replace_chain(new_chain);
        assert_eq!(chain.messages().len(), 2);
        assert_eq!(chain.messages()[0].text_content(), "new1");
        assert_eq!(chain.messages()[1].text_content(), "new2");
    }

    #[test]
    fn test_from_flat_messages_empty() {
        let chain = ChatChain::from_flat_messages(Vec::new());
        assert!(chain.is_empty());
        assert_eq!(chain.active_segments().len(), 0);
    }

    #[test]
    fn test_from_flat_messages_single_segment() {
        // from_flat_messages 不猜测 turn 边界，全部放入单段
        let msgs = vec![user_msg("a"), asst_msg("b"), user_msg("c"), asst_msg("d")];
        let chain = ChatChain::from_flat_messages(msgs);
        assert_eq!(chain.active_segments().len(), 1);
        assert_eq!(chain.messages().len(), 4);
        assert_eq!(chain.messages()[0].text_content(), "a");
        assert_eq!(chain.messages()[3].text_content(), "d");
    }
}
