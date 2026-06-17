//! Reasoning/Thinking delta 归一化器
//!
//! 某些 provider（如 Mimo）的 SSE 流中 `reasoning_content` / `thinking` 字段
//! 语义不统一，可能为以下三种模式之一：
//!
//! 1. **真增量 delta**：每个 chunk 只含新增内容，如 `["A","B","C"]` → 最终 `ABC`
//! 2. **累计全量 snapshot**：每个 chunk 含从头到当前的完整内容，如 `["A","AB","ABC"]` → 最终 `ABC`
//! 3. **重发/重复片段**：同一片段被重复发送，如 `["A","A","B"]` 或 `["ABC","ABC"]`
//!
//! 本模块提供 [`ReasoningDeltaNormalizer`]，统一处理三种模式，输出「应发送给 UI
//! 与应追加到最终 message 的净增量」，避免重复内容。

/// 归一化器对单个 chunk 采取的去重动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupAction {
    /// 无去重 — raw delta 是真增量，原样通过。
    None,
    /// snapshot 检测命中 — raw 以已累计内容为前缀，只取后缀增量。
    SnapshotSuffix,
    /// 完整重复 — raw 与已累计内容相同，丢弃。
    DuplicateDrop,
    /// 重叠裁剪 — raw 末尾与已累计内容末尾重叠，裁剪重叠部分后追加。
    OverlapTrim,
}

/// 单个 chunk 的归一化结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedReasoningDelta<'a> {
    /// 归一化后应追加的净增量（可能为空）。
    pub delta: &'a str,
    /// 归一化器采取的动作。
    pub action: DedupAction,
}

/// 每种 DedupAction 的累计命中次数，用于诊断日志。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DedupStats {
    pub none: u32,
    pub snapshot_suffix: u32,
    pub duplicate_drop: u32,
    pub overlap_trim: u32,
}

impl DedupStats {
    pub fn record(&mut self, action: DedupAction) {
        match action {
            DedupAction::None => self.none += 1,
            DedupAction::SnapshotSuffix => self.snapshot_suffix += 1,
            DedupAction::DuplicateDrop => self.duplicate_drop += 1,
            DedupAction::OverlapTrim => self.overlap_trim += 1,
        }
    }
}

/// Reasoning/Thinking delta 归一化器
///
/// 用法：
/// ```ignore
/// let mut normalizer = ReasoningDeltaNormalizer::new();
/// let result = normalizer.process(raw_delta);
/// let delta = result.delta; // 净增量，用于 handler.on_thinking(delta) + current_reasoning.push_str(delta)
/// ```
pub struct ReasoningDeltaNormalizer {
    /// 已累计的 reasoning 全文（用于 snapshot / 重复检测）。
    accumulated: String,
    /// 各 DedupAction 的累计命中次数。
    pub stats: DedupStats,
}

impl Default for ReasoningDeltaNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

/// 用于诊断的 preview 参数：首尾各记录的字符数。
const PREVIEW_CHARS: usize = 60;

/// 生成 reasoning 内容的安全 preview（首尾各 `PREVIEW_CHARS` 字符）。
pub fn safe_preview(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= PREVIEW_CHARS * 2 {
        return text.to_string();
    }
    let head: String = chars.iter().take(PREVIEW_CHARS).collect();
    let tail: String = chars.iter().skip(chars.len() - PREVIEW_CHARS).collect();
    format!("{}…{}", head, tail)
}

impl ReasoningDeltaNormalizer {
    pub fn new() -> Self {
        Self {
            accumulated: String::new(),
            stats: DedupStats::default(),
        }
    }

    /// 返回已累计的 reasoning 全文。
    pub fn accumulated(&self) -> &str {
        &self.accumulated
    }

    /// 返回已累计的 reasoning 字符数（Unicode char count）。
    pub fn accumulated_char_count(&self) -> usize {
        self.accumulated.chars().count()
    }

    /// 返回已累计的 reasoning 字节数。
    pub fn accumulated_byte_count(&self) -> usize {
        self.accumulated.len()
    }

    /// 处理一个 raw reasoning delta chunk，返回归一化后的净增量。
    ///
    /// 算法（按优先级判断）：
    /// 1. **空输入** → 不动作，返回空 delta。
    /// 2. **第一次输入**（accumulated 为空）→ 真增量，原样通过。
    /// 3. **snapshot 检测**：raw 以 accumulated 为前缀 → 只取后缀增量（`SnapshotSuffix`）。
    /// 4. **完整重复**：raw 与 accumulated 相同 → 丢弃（`DuplicateDrop`）。
    /// 5. **overlap 检测**：raw 开头与 accumulated 结尾有重叠 → 裁剪重叠部分后追加（`OverlapTrim`）。
    /// 6. **默认**：真增量，原样通过（`None`）。
    pub fn process<'a>(&mut self, raw: &'a str) -> NormalizedReasoningDelta<'a> {
        if raw.is_empty() {
            return NormalizedReasoningDelta {
                delta: "",
                action: DedupAction::None,
            };
        }

        let acc = &self.accumulated;

        // 第一次输入，accumulated 为空 — 真增量。
        if acc.is_empty() {
            self.stats.record(DedupAction::None);
            self.accumulated.push_str(raw);
            // accumulated 刚被修改，无法返回引用它的 slice；
            // 我们需要返回 raw 本身（它的生命周期由 caller 保证）。
            return NormalizedReasoningDelta {
                delta: raw,
                action: DedupAction::None,
            };
        }

        // snapshot 检测：raw 以 accumulated 为前缀（raw 包含完整历史 + 新增后缀）。
        if let Some(suffix) = raw.strip_prefix(acc.as_str()) {
            if suffix.is_empty() {
                // raw 完全等于 accumulated — 完整重复
                self.stats.record(DedupAction::DuplicateDrop);
                return NormalizedReasoningDelta {
                    delta: "",
                    action: DedupAction::DuplicateDrop,
                };
            }
            // snapshot：取后缀增量
            self.stats.record(DedupAction::SnapshotSuffix);
            self.accumulated.push_str(suffix);
            // suffix 是 raw 的子切片（raw = prefix + suffix），生命周期安全。
            return NormalizedReasoningDelta {
                delta: suffix,
                action: DedupAction::SnapshotSuffix,
            };
        }

        // overlap 检测：raw 开头与 accumulated 结尾重叠。
        // 例：acc="Hello Wor", raw="World!" → 重叠="Wor", 净增量="ld!"
        if let Some(trimmed) = trim_overlap(acc, raw) {
            self.stats.record(DedupAction::OverlapTrim);
            self.accumulated.push_str(trimmed.delta);
            return NormalizedReasoningDelta {
                delta: trimmed.delta,
                action: DedupAction::OverlapTrim,
            };
        }

        // 默认：真增量
        self.stats.record(DedupAction::None);
        self.accumulated.push_str(raw);
        NormalizedReasoningDelta {
            delta: raw,
            action: DedupAction::None,
        }
    }
}

/// 检测 `accumulated` 末尾与 `raw` 开头的最大重叠，返回裁剪后的净增量。
///
/// 例：`accumulated = "Hello Wor"`, `raw = "World!"`
/// → 重叠 = `"Wor"`, 净增量 = `"ld!"`
///
/// 为避免误伤短字符串的正常重复，设置最小重叠长度阈值。
const MIN_OVERLAP_LEN: usize = 3;

fn trim_overlap<'a>(accumulated: &str, raw: &'a str) -> Option<NormalizedReasoningDelta<'a>> {
    let acc_chars: Vec<char> = accumulated.chars().collect();
    let raw_chars: Vec<char> = raw.chars().collect();

    let max_possible = acc_chars.len().min(raw_chars.len());
    if max_possible < MIN_OVERLAP_LEN {
        return None;
    }

    for overlap_len in (MIN_OVERLAP_LEN..=max_possible).rev() {
        let acc_tail = &acc_chars[acc_chars.len() - overlap_len..];
        let raw_head = &raw_chars[..overlap_len];
        if acc_tail == raw_head {
            // 净增量 = raw 去掉开头 overlap_len 个字符（按 char 偏移转 byte 偏移）
            let byte_offset: usize = raw
                .char_indices()
                .nth(overlap_len)
                .map(|(idx, _)| idx)
                .unwrap_or(raw.len());
            let delta_str = &raw[byte_offset..];
            if delta_str.is_empty() {
                return None; // 完全重叠 = DuplicateDrop 已在 process 中处理
            }
            return Some(NormalizedReasoningDelta {
                delta: delta_str,
                action: DedupAction::OverlapTrim,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- issue 验证建议的 4 种场景 ---

    #[test]
    fn test_process_true_delta_abc() {
        // 场景1：provider 返回真增量 ["A","B","C"] → 输出 ABC
        let mut n = ReasoningDeltaNormalizer::new();
        let r1 = n.process("A");
        let r2 = n.process("B");
        let r3 = n.process("C");
        let total = format!("{}{}{}", r1.delta, r2.delta, r3.delta);
        assert_eq!(total, "ABC");
        assert_eq!(n.accumulated(), "ABC");
        assert_eq!(r1.action, DedupAction::None);
        assert_eq!(r2.action, DedupAction::None);
        assert_eq!(r3.action, DedupAction::None);
        assert_eq!(n.stats.none, 3);
        assert_eq!(n.stats.snapshot_suffix, 0);
    }

    #[test]
    fn test_process_snapshot_a_ab_abc() {
        // 场景2：provider 返回累计全量 ["A","AB","ABC"] → 输出 ABC
        let mut n = ReasoningDeltaNormalizer::new();
        let r1 = n.process("A");
        let r2 = n.process("AB");
        let r3 = n.process("ABC");
        let total = format!("{}{}{}", r1.delta, r2.delta, r3.delta);
        assert_eq!(total, "ABC");
        assert_eq!(n.accumulated(), "ABC");
        assert_eq!(r1.action, DedupAction::None);
        assert_eq!(r2.action, DedupAction::SnapshotSuffix);
        assert_eq!(r3.action, DedupAction::SnapshotSuffix);
        assert_eq!(n.stats.snapshot_suffix, 2);
    }

    #[test]
    fn test_process_duplicate_resend_a_a_b() {
        // 场景3a：provider 重发重复片段 ["A","A","B"] → 不重复输出 AB
        let mut n = ReasoningDeltaNormalizer::new();
        let r1 = n.process("A");
        let r2 = n.process("A");
        let r3 = n.process("B");
        let total = format!("{}{}{}", r1.delta, r2.delta, r3.delta);
        assert_eq!(total, "AB");
        assert_eq!(n.accumulated(), "AB");
        assert_eq!(r1.action, DedupAction::None);
        assert_eq!(r2.action, DedupAction::DuplicateDrop);
        assert_eq!(r3.action, DedupAction::None);
        assert_eq!(n.stats.duplicate_drop, 1);
    }

    #[test]
    fn test_process_duplicate_resend_abc_abc() {
        // 场景3b：provider 重发完整片段 ["ABC","ABC"] → 不重复输出 ABC
        let mut n = ReasoningDeltaNormalizer::new();
        let r1 = n.process("ABC");
        let r2 = n.process("ABC");
        let total = format!("{}{}", r1.delta, r2.delta);
        assert_eq!(total, "ABC");
        assert_eq!(n.accumulated(), "ABC");
        assert_eq!(r1.action, DedupAction::None);
        assert_eq!(r2.action, DedupAction::DuplicateDrop);
    }

    // --- 边界条件 ---

    #[test]
    fn test_process_empty_input() {
        let mut n = ReasoningDeltaNormalizer::new();
        let r = n.process("");
        assert_eq!(r.delta, "");
        assert_eq!(r.action, DedupAction::None);
        assert_eq!(n.accumulated(), "");
    }

    #[test]
    fn test_process_empty_after_nonempty() {
        let mut n = ReasoningDeltaNormalizer::new();
        n.process("Hello");
        let r = n.process("");
        assert_eq!(r.delta, "");
        assert_eq!(r.action, DedupAction::None);
        assert_eq!(n.accumulated(), "Hello");
    }

    #[test]
    fn test_process_unicode_true_delta() {
        // Unicode 真增量（中日韩）
        let mut n = ReasoningDeltaNormalizer::new();
        let r1 = n.process("你好");
        let r2 = n.process("世界");
        let total = format!("{}{}", r1.delta, r2.delta);
        assert_eq!(total, "你好世界");
        assert_eq!(n.accumulated(), "你好世界");
    }

    #[test]
    fn test_process_unicode_snapshot() {
        // Unicode snapshot：每个 chunk 含完整历史
        let mut n = ReasoningDeltaNormalizer::new();
        let r1 = n.process("用户的问题");
        let r2 = n.process("用户的问题可能是指");
        let r3 = n.process("用户的问题可能是指有些 tool");
        let total = format!("{}{}{}", r1.delta, r2.delta, r3.delta);
        assert_eq!(total, "用户的问题可能是指有些 tool");
        assert_eq!(n.accumulated(), "用户的问题可能是指有些 tool");
    }

    #[test]
    fn test_process_mixed_snapshot_and_delta() {
        // 混合模式：先 snapshot 后真 delta
        let mut n = ReasoningDeltaNormalizer::new();
        let r1 = n.process("分析开始。");
        let r2 = n.process("分析开始。检查代码"); // snapshot
        let r3 = n.process("路径。"); // 真增量
        let total = format!("{}{}{}", r1.delta, r2.delta, r3.delta);
        assert_eq!(total, "分析开始。检查代码路径。");
        assert_eq!(n.accumulated(), "分析开始。检查代码路径。");
    }

    // --- overlap trim ---

    #[test]
    fn test_process_overlap_trim() {
        // acc 末尾与 raw 开头重叠
        let mut n = ReasoningDeltaNormalizer::new();
        n.process("Hello Wor");
        let r = n.process("World!");
        assert_eq!(r.delta, "ld!");
        assert_eq!(r.action, DedupAction::OverlapTrim);
        assert_eq!(n.accumulated(), "Hello World!");
    }

    #[test]
    fn test_process_overlap_trim_short_no_false_positive() {
        // 重叠长度 < MIN_OVERLAP_LEN，不触发 overlap trim，当真增量处理
        let mut n = ReasoningDeltaNormalizer::new();
        n.process("AB");
        let r = n.process("BC"); // max_possible=2 < MIN_OVERLAP_LEN=3，不检测
        assert_eq!(r.action, DedupAction::None);
        assert_eq!(n.accumulated(), "ABBC"); // 原样追加
    }

    // --- safe_preview ---

    #[test]
    fn test_safe_preview_short() {
        assert_eq!(safe_preview("hello"), "hello");
    }

    #[test]
    fn test_safe_preview_long() {
        let long = "a".repeat(200);
        let preview = safe_preview(&long);
        assert!(preview.contains('…'));
        // head 60 chars + … + tail 60 chars
        let head: String = preview.chars().take(60).collect();
        assert_eq!(head, "a".repeat(60));
    }

    #[test]
    fn test_safe_preview_exact_boundary() {
        // 恰好 120 字符（PREVIEW_CHARS * 2），不截断
        let exact = "x".repeat(120);
        assert_eq!(safe_preview(&exact), exact);
    }

    // --- DedupStats ---

    #[test]
    fn test_dedup_stats_record() {
        let mut stats = DedupStats::default();
        stats.record(DedupAction::None);
        stats.record(DedupAction::None);
        stats.record(DedupAction::SnapshotSuffix);
        stats.record(DedupAction::DuplicateDrop);
        stats.record(DedupAction::DuplicateDrop);
        stats.record(DedupAction::OverlapTrim);
        assert_eq!(stats.none, 2);
        assert_eq!(stats.snapshot_suffix, 1);
        assert_eq!(stats.duplicate_drop, 2);
        assert_eq!(stats.overlap_trim, 1);
    }

    // --- 回归：issue 现场模拟 ---

    #[test]
    fn test_regression_mimo_repeated_thinking() {
        // 模拟 issue 现场的关键词重复：同一片段在多个 chunk 中重复出现
        let mut n = ReasoningDeltaNormalizer::new();

        // 第一个 chunk：正常增量
        let r1 = n.process("用户的问题可能是指：有些 tool 的 tool name 没有渲染成颜色");
        assert_eq!(r1.action, DedupAction::None);

        // 第二个 chunk：重复发送相同内容（Mimo 服务端重复 / stream 层 snapshot）
        let r2 = n.process("用户的问题可能是指：有些 tool 的 tool name 没有渲染成颜色");
        assert_eq!(r2.action, DedupAction::DuplicateDrop);
        assert_eq!(r2.delta, "");

        // 第三个 chunk：snapshot 包含前面全部内容 + 新增
        let r3 = n.process(
            "用户的问题可能是指：有些 tool 的 tool name 没有渲染成颜色。让我检查一下代码。",
        );
        assert_eq!(r3.action, DedupAction::SnapshotSuffix);
        assert_eq!(r3.delta, "。让我检查一下代码。");

        // 最终 accumulated 无重复
        assert_eq!(
            n.accumulated(),
            "用户的问题可能是指：有些 tool 的 tool name 没有渲染成颜色。让我检查一下代码。"
        );
        // 统计
        assert_eq!(n.stats.none, 1);
        assert_eq!(n.stats.duplicate_drop, 1);
        assert_eq!(n.stats.snapshot_suffix, 1);
    }
}
