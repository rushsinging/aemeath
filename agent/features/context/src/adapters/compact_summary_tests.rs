use super::*;
use std::sync::Arc;

/// #1486：分块目标按上下文总长度比例切（context_size / 8），
/// 带上下限保护，不固定 30k。
#[test]
fn chunk_target_scales_with_context_size() {
    use crate::domain::token_budget::compact_chunk_target_tokens;

    // 中窗口：272000 / 8 = 34000
    assert_eq!(compact_chunk_target_tokens(272_000), 34_000);
    // 小窗口：128000 / 8 = 16000
    assert_eq!(compact_chunk_target_tokens(128_000), 16_000);
    // 超大窗口：上限 40k（防止单块摘要请求超 provider 输入限制）
    assert_eq!(compact_chunk_target_tokens(1_048_576), 40_000);
    // 极小窗口：下限 8k（太小没有分块意义）
    assert_eq!(compact_chunk_target_tokens(32_000), 8_000);
    assert_eq!(compact_chunk_target_tokens(64_000), 8_000);
}

/// 分块数量应随 context_size 变化：同量消息，窗口越大块数越少。
#[tokio::test]
async fn map_reduce_chunk_count_follows_context_size_ratio() {
    use crate::domain::token_budget::compact_chunk_target_tokens;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "这是一个用于触发分块压缩的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    struct CountingGenerator {
        calls: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl CompactGenerator for CountingGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let text = request
                .first()
                .map(|msg| msg.text_content())
                .unwrap_or_default();
            Ok(format!("<summary>chunk of {} chars</summary>", text.len()))
        }
    }

    // 小窗口（64k → target 8k）：块数多
    let small_calls = Arc::new(AtomicUsize::new(0));
    let small = compact_messages_with_llm(
        &messages,
        None,
        64_000,
        Some(&CountingGenerator {
            calls: small_calls.clone(),
        }),
        None,
        &cancel,
    )
    .await
    .expect("compact should run");
    let small_chunks = small_calls.load(Ordering::SeqCst);

    // 大窗口（1M → target 40k，clamp 上限）：块数少
    let large_calls = Arc::new(AtomicUsize::new(0));
    let large = compact_messages_with_llm(
        &messages,
        None,
        1_048_576,
        Some(&CountingGenerator {
            calls: large_calls.clone(),
        }),
        None,
        &cancel,
    )
    .await
    .expect("compact should run");
    let large_chunks = large_calls.load(Ordering::SeqCst);

    assert!(
        small_chunks > large_chunks,
        "窗口越小块数应越多: small={small_chunks} large={large_chunks} (target small={}, large={})",
        compact_chunk_target_tokens(64_000),
        compact_chunk_target_tokens(1_048_576),
    );
    let _ = (small, large);
}

#[test]
fn compact_execution_does_not_repeat_threshold_decision() {
    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();

    let result = compact_messages(&messages);

    assert!(
        result.is_some(),
        "上层以归一化 API token 决定 compact 后，执行管线不得再用纯估算否决"
    );
}

#[test]
fn test_compact_window_boundaries() {
    assert_eq!(compact_window(4), None);
    assert_eq!(compact_window(5), None);
    assert_eq!(compact_window(6), None);

    assert_eq!(
        compact_window(100),
        Some(CompactWindow {
            head_protect: 2,
            split_point: 90,
            keep_recent: 10,
        })
    );
}

#[test]
fn test_messages_selected_for_precompact_memory_uses_same_early_window_as_compact() {
    let messages = (0..10)
        .map(|idx| Message::user(format!("message-{idx}")))
        .collect::<Vec<_>>();

    let selected = messages_selected_for_precompact_memory(&messages);

    let selected_text = selected
        .iter()
        .map(Message::text_content)
        .collect::<Vec<_>>();
    assert_eq!(
        selected_text,
        vec![
            "message-0",
            "message-1",
            "message-2",
            "message-3",
            "message-4",
            "message-5",
        ]
    );
}

#[test]
fn test_messages_selected_for_precompact_memory_returns_empty_for_small_history() {
    let messages = vec![
        Message::user("one"),
        Message::user("two"),
        Message::user("three"),
        Message::user("four"),
    ];

    assert!(messages_selected_for_precompact_memory(&messages).is_empty());
}

#[test]
fn compact_prompt_preserves_user_requests_and_continuation_state() {
    assert!(COMPACT_PROMPT.contains("## User Requests"));
    assert!(COMPACT_PROMPT.contains("## Work Completed"));
    assert!(COMPACT_PROMPT.contains("## Problems / Findings"));
    assert!(COMPACT_PROMPT.contains("## Next Action"));
    assert!(COMPACT_PROMPT.contains("## Continuation Status"));
    assert!(COMPACT_PROMPT.contains("later corrections supersede"));
    assert!(COMPACT_PROMPT.contains("NEVER upgrade"));
}

#[test]
fn compact_request_contains_all_user_inputs_in_order() {
    let request = build_compact_request(
        &[
            Message::user("看看 issue 850"),
            Message::user("只分析，不实现"),
            Message::user("按 segment 汇总"),
        ],
        None,
        100_000,
    );
    let text = request[0].text_content();
    let inspect = text.find("看看 issue 850").unwrap();
    let no_implementation = text.find("只分析，不实现").unwrap();
    let by_segment = text.find("按 segment 汇总").unwrap();

    assert!(inspect < no_implementation);
    assert!(no_implementation < by_segment);
}

#[test]
fn compact_request_merges_previous_summary_without_duplicate_empty_prompt() {
    let request = build_compact_request(
        &[Message::user("继续检查 compact")],
        Some("earlier user request and completed work"),
        100_000,
    );

    assert_eq!(request.len(), 1);
    let text = request[0].text_content();
    assert_eq!(
        text.matches("You are a conversation history compactor")
            .count(),
        1
    );
    assert_eq!(text.matches("<conversation_history>").count(), 1);
    assert!(text.contains("<previous_summary>"));
    assert!(text.contains("earlier user request and completed work"));
    assert!(text.contains("继续检查 compact"));
}

/// #1486：LLM 压缩请求的 previous_summary 必须截断（保留尾部），
/// 否则上次 summary 巨大时压缩请求本身超 provider 输入限制，
/// LLM 压缩必然失败、永远 fallback。
#[test]
fn compact_request_caps_oversized_previous_summary() {
    let huge_previous = "x".repeat(900_000);
    let request = build_compact_request(
        &[Message::user("继续")],
        Some(huge_previous.as_str()),
        100_000,
    );

    let text = request[0].text_content();
    let cap = crate::domain::token_budget::FALLBACK_PREVIOUS_SUMMARY_CAP;
    assert!(
        text.len() <= cap + 6_000,
        "previous_summary 超大时压缩请求必须保持有界: {} chars (cap={cap} + 模板开销)",
        text.len()
    );
    assert!(
        text.contains("<previous_summary_tail>"),
        "应使用截断标记: {text}"
    );
}

#[test]
fn fallback_summary_latest_user_request_continues_without_claiming_completion() {
    let summary = build_summary_text(
        &[
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "计划读取 issue，尚未执行".to_string(),
                }],
                metadata: None,
            },
            Message::user("只分析，不实现"),
        ],
        None,
    );

    assert!(summary.contains("## User Requests"));
    assert!(summary.contains("只分析，不实现"));
    assert!(summary.contains("## Work Completed"));
    assert!(summary.contains("Unverified assistant report"));
    assert!(!summary.contains("- 已完成"));
    assert!(summary.contains("## Problems / Findings"));
    assert!(summary.contains("## Next Action"));
    assert!(summary.contains("## Continuation Status"));
    assert!(summary.contains("Continue"));
}

#[test]
fn fallback_summary_waiting_for_approval_does_not_continue() {
    let summary = build_summary_text(
        &[Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "方案已给出，等待你确认后再修改".to_string(),
            }],
            metadata: None,
        }],
        None,
    );

    assert!(summary.contains("Waiting for User"));
    assert!(summary.contains("等待你确认"));
}

#[test]
fn fallback_summary_explicit_completion_report_waits_for_user_confirmation() {
    let summary = build_summary_text(
        &[Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "已完成代码修改并通过测试".to_string(),
            }],
            metadata: None,
        }],
        None,
    );

    assert!(summary.contains("Assistant-reported completion"));
    assert!(summary.contains("Waiting for User"));
    assert!(!summary.contains("\nCompleted —"));
}

#[test]
fn fallback_summary_negated_completion_is_not_treated_as_completed() {
    for text in [
        "work is not completed",
        "branch is not merged",
        "修改尚未完成",
        "没有合入",
    ] {
        let summary = build_summary_text(
            &[Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: text.to_string(),
                }],
                metadata: None,
            }],
            None,
        );

        assert!(
            !summary.contains("Assistant-reported completion"),
            "{text} must not be classified as completion"
        );
        assert!(summary.contains("Waiting for User"));
        assert!(!summary.contains("\nCompleted —"));
    }
}

#[tokio::test]
async fn second_compact_fallback_preserves_previous_summary() {
    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    let result = compact_messages_with_llm(
        &messages,
        Some("first compact summary with original user request"),
        100_000,
        None,
        None,
        &cancel,
    )
    .await
    .expect("second compact should run");

    assert!(
        result
            .summary
            .contains("first compact summary with original user request"),
        "second compact must retain the previous active summary"
    );
}

/// 复现 #1486：超大 previous_summary（92 万字符场景）不得全文嵌入新 summary，
/// 否则多次 compact 线性累加，最终撑爆 system prompt。
#[tokio::test]
async fn fallback_never_embeds_oversized_previous_summary_verbatim() {
    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    // 模拟真实事故：previous_summary 已达 92 万字符
    let huge_previous = "x".repeat(900_000);

    let result = compact_messages_with_llm(
        &messages,
        Some(huge_previous.as_str()),
        100_000,
        None,
        None,
        &cancel,
    )
    .await
    .expect("compact should run");

    let cap = crate::adapters::compact_summary::FALLBACK_PREVIOUS_SUMMARY_CAP;
    assert!(
        result.summary.len() <= cap + 2_000,
        "summary 必须保持有界: {} chars (cap={cap})",
        result.summary.len()
    );
}

/// 超大 previous_summary 只保留关键尾部，长度有界（#1486）。
#[test]
fn fallback_keeps_previous_summary_tail_when_oversized() {
    // 构造一个头部是噪声、尾部是关键信息的 previous_summary
    let head = "HEAD-NOISE-".repeat(50_000);
    let tail = "## Next Action\n- Continue the merge.\n\n## Continuation Status\nContinue";
    let previous = format!("{head}{tail}");

    let summary = crate::adapters::compact_summary::build_summary_text(&[], Some(&previous));

    assert!(
        summary.contains("Continue the merge."),
        "尾部关键信息必须保留: {summary}"
    );
    let cap = crate::adapters::compact_summary::FALLBACK_PREVIOUS_SUMMARY_CAP;
    assert!(
        summary.len() <= cap + 2_000,
        "previous_summary 超大时 summary 必须保持有界: {} chars (cap={cap})",
        summary.len()
    );
}

/// map-reduce 分块摘要必须并发执行（3-5 并发，视块数而定），
/// 而不是串行逐个调用（#1486）。
#[tokio::test]
async fn map_reduce_compacts_chunks_concurrently_with_bounded_parallelism() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // 构造足够大的消息集触发 map-reduce（> COMPACT_CHUNK_TARGET_TOKENS），
    // 每条消息约 100 个 CJK 字符 → 单条约 ~270 tokens，600 条 ≈ 16 万 tokens → 5+ 块
    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "这是一个用于触发分块压缩的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    let current = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let call_count = Arc::new(AtomicUsize::new(0));

    struct ObservingGenerator {
        current: Arc<AtomicUsize>,
        max_concurrent: Arc<AtomicUsize>,
        call_count: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl CompactGenerator for ObservingGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<String, String> {
            let active = self.current.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_concurrent.fetch_max(active, Ordering::SeqCst);
            self.call_count.fetch_add(1, Ordering::SeqCst);
            // 让并发窗口真实展开
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let text = request
                .first()
                .map(|msg| msg.text_content())
                .unwrap_or_default();
            self.current.fetch_sub(1, Ordering::SeqCst);
            Ok(format!(
                "<summary>chunk summary: {}</summary>",
                &text[..text.floor_char_boundary(30)]
            ))
        }
    }

    let generator = ObservingGenerator {
        current,
        max_concurrent,
        call_count,
    };

    let result =
        compact_messages_with_llm(&messages, None, 100_000, Some(&generator), None, &cancel)
            .await
            .expect("map-reduce compact should run");

    let chunks = generator.call_count.load(Ordering::SeqCst);
    assert!(
        chunks >= 3,
        "消息集应产生至少 3 个 chunk（map 阶段调用次数），实际 {chunks}"
    );
    assert!(
        generator.max_concurrent.load(Ordering::SeqCst) >= 2,
        "map 阶段必须并发执行，实际最大并发 {}",
        generator.max_concurrent.load(Ordering::SeqCst)
    );
    assert!(
        generator.max_concurrent.load(Ordering::SeqCst) <= 5,
        "map 阶段并发不得超过 5，实际 {}",
        generator.max_concurrent.load(Ordering::SeqCst)
    );
    assert!(
        result.summary.contains("chunk summary"),
        "reduce 合并结果应来自子摘要: {}",
        result.summary
    );
}

/// 汇总后的最终摘要若超过预算，必须再压一次（收敛迭代，#1486）。
#[tokio::test]
async fn reduce_compresses_again_when_final_summary_exceeds_budget() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    // 与并发测试相同规模的 600 条消息，确保触发 map-reduce（5+ 块）
    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "触发分块压缩的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    let call_count = Arc::new(AtomicUsize::new(0));
    let seen_reduce = Arc::new(AtomicBool::new(false));

    /// 阶段区分（map/reduce/refresh 串行执行，安全）：
    /// - reduce（含 "sub-summaries"）→ 标记 seen_reduce，返回超长摘要
    /// - map / refresh（含 "conversation_history"）→ 返回短摘要
    /// 最终 summary 来自 refresh 的收敛结果。
    struct ShrinkingGenerator {
        call_count: Arc<AtomicUsize>,
        seen_reduce: Arc<AtomicBool>,
    }
    #[async_trait::async_trait]
    impl CompactGenerator for ShrinkingGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<String, String> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let text = request
                .first()
                .map(|msg| msg.text_content())
                .unwrap_or_default();
            if text.contains("sub-summaries") {
                self.seen_reduce.store(true, Ordering::SeqCst);
                // reduce：返回超长摘要（模拟 LLM 不听预算）
                Ok(format!("<summary>{}</summary>", "y".repeat(80_000)))
            } else {
                Ok("<summary>收敛后的最终摘要</summary>".to_string())
            }
        }
    }

    let generator = ShrinkingGenerator {
        call_count,
        seen_reduce,
    };

    let result =
        compact_messages_with_llm(&messages, None, 100_000, Some(&generator), None, &cancel)
            .await
            .expect("compact should run");

    assert_eq!(result.summary, "收敛后的最终摘要");
    assert!(
        generator.seen_reduce.load(Ordering::SeqCst),
        "reduce 阶段必须发生"
    );
    assert!(
        generator.call_count.load(Ordering::SeqCst) >= 6,
        "超预算时必须再压，调用次数应更多（map 5+ 块 + reduce + 再压）: {}",
        generator.call_count.load(Ordering::SeqCst)
    );
}

/// Mock `CompactGenerator` that returns a canned response regardless of input.
struct MockGenerator {
    text: String,
}

#[async_trait::async_trait]
impl CompactGenerator for MockGenerator {
    async fn generate(
        &self,
        _request: Vec<Message>,
        _cancel: &CancellationToken,
    ) -> Result<String, String> {
        Ok(format!("<summary>{}</summary>", self.text))
    }
}

#[tokio::test]
async fn compact_with_generator_uses_llm_summary() {
    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    let generator = MockGenerator {
        text: "LLM summary of early conversation".to_string(),
    };

    let result =
        compact_messages_with_llm(&messages, None, 100_000, Some(&generator), None, &cancel)
            .await
            .expect("compact should run");

    // The summary should come from the generator, not the fallback text.
    assert_eq!(result.summary, "LLM summary of early conversation");
    assert!(!result.summary.contains("deterministic fallback"));
}

#[tokio::test]
async fn compact_falls_back_when_generator_errors() {
    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    struct FailingGenerator;
    #[async_trait::async_trait]
    impl CompactGenerator for FailingGenerator {
        async fn generate(
            &self,
            _request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<String, String> {
            Err("simulated LLM failure".to_string())
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&FailingGenerator),
        None,
        &cancel,
    )
    .await
    .expect("compact should still run with fallback");

    // Fallback summary 使用本地压缩模板（#1486：文案如实描述本地路径）。
    assert!(result.summary.contains("## User Requests"));
    assert!(result.summary.contains("Local text-compaction path"));
    assert!(!result.summary.contains("Semantic LLM compaction failed"));
}

/// #1490：再压提示词必须使用缩减预算（summary_budget × 0.8）并硬约束，
/// 替代通用 COMPACT_PROMPT 的 "more detail is better" 反效果措辞。
#[test]
fn refresh_prompt_enforces_shrunk_budget() {
    use crate::domain::token_budget::summary_budget;

    let budget = summary_budget(272_000); // 5440
    let prompt = crate::adapters::compact_summary::build_refresh_prompt(
        "## User Requests\n- 继续 issue",
        budget,
    );

    assert!(
        prompt.contains("MUST NOT exceed"),
        "再压提示词必须有硬预算约束: {prompt}"
    );
    let expected_prompt_budget = budget * 8 / 10; // ×0.8 留余量
    assert!(
        prompt.contains(&expected_prompt_budget.to_string()),
        "提示预算应为 summary_budget×0.8（{expected_prompt_budget}）: {prompt}"
    );
    assert!(
        !prompt.contains("More detail is better"),
        "再压不应使用通用模板的鼓励细节措辞"
    );
    assert!(prompt.contains("## User Requests"));
}

/// #1490：收敛判定容忍一轮噪音——第一轮未缩小继续，第二轮才停；
/// 且未缩小轮次不采用更差输出。
#[tokio::test]
async fn refresh_stops_after_two_non_shrinking_rounds_without_worsening() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    // 构造足以触发 map-reduce 的消息
    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "触发分块压缩的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    let reduce_seen = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));

    /// reduce 返回超长摘要；refresh 阶段始终返回与输入等长（未缩小）的摘要。
    struct NeverShrinkingGenerator {
        reduce_seen: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl CompactGenerator for NeverShrinkingGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let text = request
                .first()
                .map(|msg| msg.text_content())
                .unwrap_or_default();
            if text.contains("sub-summaries") {
                self.reduce_seen.store(true, Ordering::SeqCst);
                Ok(format!("<summary>{}</summary>", "y".repeat(40_000)))
            } else if text.contains("compress an existing conversation summary") {
                // refresh：返回与输入等长的摘要（模拟 LLM 不缩小）
                let input = text
                    .find("<current_summary>")
                    .and_then(|start| text.find("</current_summary>").map(|end| &text[start..end])) // allow unsafe_text_op: find offset (char boundary)
                    .unwrap_or("");
                Ok(format!("<summary>{input}</summary>"))
            } else {
                Ok("<summary>chunk</summary>".to_string())
            }
        }
    }

    let generator = NeverShrinkingGenerator {
        reduce_seen: reduce_seen.clone(),
        calls: calls.clone(),
    };
    let result =
        compact_messages_with_llm(&messages, None, 100_000, Some(&generator), None, &cancel)
            .await
            .expect("compact should run");

    assert!(reduce_seen.load(Ordering::SeqCst), "reduce 阶段必须发生");
    assert!(
        result.summary.len() < 41_000,
        "未缩小轮次不应采用更差输出（保持原 reduce 结果）: {} chars",
        result.summary.len()
    );
    assert!(
        calls.load(Ordering::SeqCst) >= 3,
        "至少 reduce + 两轮 refresh: {}",
        calls.load(Ordering::SeqCst)
    );
}

/// #1500：progress 回调必须收到完整阶段序列——Preparing →
/// Summarizing（map-reduce 带 chunk 计数 1..N）→ Finalizing。
#[tokio::test]
async fn progress_callback_receives_stages_and_chunk_counts() {
    use std::sync::{Arc, Mutex};

    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "触发分块压缩的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();
    let seen = Arc::new(Mutex::new(Vec::<(String, CompactWork)>::new()));

    struct EchoGenerator;
    #[async_trait::async_trait]
    impl CompactGenerator for EchoGenerator {
        async fn generate(
            &self,
            _request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<String, String> {
            Ok("<summary>ok</summary>".to_string())
        }
    }

    let progress = {
        let seen = seen.clone();
        move |stage: CompactStage, work: CompactWork| {
            seen.lock()
                .unwrap()
                .push((stage.as_str().to_string(), work));
        }
    };

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&EchoGenerator),
        Some(&progress),
        &cancel,
    )
    .await
    .expect("compact should run");
    assert!(result.summary.contains("ok"));

    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.first().map(|(stage, _)| stage.as_str()),
        Some("preparing"),
        "首个进度必须是 preparing，实际 {seen:?}"
    );
    assert_eq!(
        seen.last().map(|(stage, _)| stage.as_str()),
        Some("finalizing"),
        "末个进度必须是 finalizing，实际 {seen:?}"
    );
    let mapping: Vec<_> = seen
        .iter()
        .filter(|(stage, _)| stage == "mapping")
        .collect();
    assert!(
        mapping.len() >= 3,
        "map-reduce 应按真实完成上报多个 chunk 进度，实际 {mapping:?}"
    );
    let CompactWork::Determinate {
        completed: first_completed,
        total: first_total,
    } = mapping[0].1
    else {
        panic!("mapping 必须是 determinate work")
    };
    let CompactWork::Determinate {
        completed: last_completed,
        total: last_total,
    } = mapping.last().unwrap().1
    else {
        panic!("mapping 必须是 determinate work")
    };
    assert_eq!(first_completed, 1, "chunk 完成计数应从 1 开始");
    assert_eq!(first_total, last_total, "total 必须全程一致");
    assert_eq!(last_completed, last_total, "最后完成计数应等于 total");
    assert!(
        seen.iter().any(|(stage, _)| stage == "reducing"),
        "mapping 完成后必须进入 reducing，实际 {seen:?}"
    );
}

/// #1500：单次摘要（非 map-reduce）progress 为阶段事件，chunk 计数为 None。
#[tokio::test]
async fn progress_callback_single_summary_reports_stages_without_chunk_counts() {
    use std::sync::{Arc, Mutex};

    let messages = (0..8)
        .map(|index| Message::user(format!("短会话消息编号 {index}，不足以触发 map-reduce。")))
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();
    let seen = Arc::new(Mutex::new(Vec::<(String, CompactWork)>::new()));

    struct EchoGenerator;
    #[async_trait::async_trait]
    impl CompactGenerator for EchoGenerator {
        async fn generate(
            &self,
            _request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<String, String> {
            Ok("<summary>ok</summary>".to_string())
        }
    }

    let progress = {
        let seen = seen.clone();
        move |stage: CompactStage, work: CompactWork| {
            seen.lock()
                .unwrap()
                .push((stage.as_str().to_string(), work));
        }
    };

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&EchoGenerator),
        Some(&progress),
        &cancel,
    )
    .await
    .expect("compact should run");
    assert!(result.summary.contains("ok"));

    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.iter()
            .map(|(stage, _)| stage.as_str())
            .collect::<Vec<_>>(),
        vec!["preparing", "generating", "finalizing"],
        "单次摘要进度序列，实际 {seen:?}"
    );
    assert!(
        seen.iter()
            .all(|(_, work)| *work == CompactWork::Indeterminate),
        "单次摘要不应伪造 chunk 计数，实际 {seen:?}"
    );
}
