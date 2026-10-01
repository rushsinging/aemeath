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
    let r3 =
        n.process("用户的问题可能是指：有些 tool 的 tool name 没有渲染成颜色。让我检查一下代码。");
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
