use super::*;
use std::sync::Arc;

const VALID_MAP_FACTS: &str = r#"{
  "facts": [
    {
      "sequence": 1,
      "source": "main_user",
      "kind": "constraint",
      "text": "NEVER widen the requested action level.",
      "constraint": {
        "scope": "session",
        "lifecycle": "persistent",
        "action": "restrict"
      }
    },
    {
      "sequence": 2,
      "source": "main_user",
      "kind": "objective",
      "text": "Continue the compact checkpoint work."
    },
    {
      "sequence": 3,
      "source": "main_user",
      "kind": "resume_candidate",
      "text": "Validate the generated checkpoint."
    }
  ]
}"#;

const SHORTER_COMPRESSION_PATCH: &str = r#"{
  "committed_facts": [],
  "uncommitted_working_set": [],
  "resume_context": [],
  "required_revalidation": ["Recheck worktree and CI state before delivery."],
  "archived_milestones": []
}"#;

const VALID_CHECKPOINT_WIRE: &str = r#"{
  "immutable_constraints": ["NEVER widen the requested action level."],
  "current_objective": "Continue the compact checkpoint work.",
  "committed_facts": ["Existing compact tests passed before this change."],
  "uncommitted_working_set": ["Checkpoint normalization is in progress."],
  "open_decisions_and_risks": ["Provider output may violate the schema."],
  "resume_cursor": {
    "context": [],
    "next_action": "validate the generated checkpoint.",
    "prohibited_actions": ["do not merge without user approval."]
  },
  "required_revalidation": ["Recheck worktree and CI state before delivery."],
  "archived_milestones": ["Previous summary contract completed in `#671`."],
  "continuation_status": "continue",
  "continuation_reason": "checkpoint normalization remains."
}"#;

fn typed_response_for_request(request: &[Message]) -> String {
    let text = request
        .first()
        .map(Message::text_content)
        .unwrap_or_default();
    if text.contains("<compact_facts>") {
        VALID_CHECKPOINT_WIRE.to_string()
    } else if text.contains("<unprotected_checkpoint_details>") {
        SHORTER_COMPRESSION_PATCH.to_string()
    } else {
        VALID_MAP_FACTS.to_string()
    }
}

const VALID_CHECKPOINT: &str = r#"## Immutable Constraints
- NEVER widen the requested action level.

## Current Objective
- Continue the compact checkpoint work.

## Committed Facts
- Existing compact tests passed before this change.

## Uncommitted Working Set
- Checkpoint normalization is in progress.

## Open Decisions / Risks
- Provider output may violate the schema.

## Resume Cursor
- Next action: validate the generated checkpoint.
- Prohibited: do not merge without user approval.

## Required Revalidation
- Recheck worktree and CI state before delivery.

## Archived Milestones
- Previous summary contract completed in `#671`.

## Continuation Status
Continue — checkpoint normalization remains."#;

fn oversized_valid_checkpoint(noise_len: usize) -> String {
    VALID_CHECKPOINT.replace(
        "- Previous summary contract completed in `#671`.",
        &format!(
            "- Previous summary contract completed in `#671`.\n- {}",
            "archive-noise ".repeat(noise_len / 14)
        ),
    )
}

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

/// Map 分块目标只跟随 compact 调用模型窗口，与 summary 注入窗口无关。
#[tokio::test]
async fn map_reduce_chunk_target_follows_compact_model_window() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingGenerator {
        calls: Arc<AtomicUsize>,
        window: Option<usize>,
    }
    #[async_trait::async_trait]
    impl CompactGenerator for CountingGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(CompactGenerationOutputData::from(
                typed_response_for_request(&request),
            ))
        }

        async fn compact_context_window(&self) -> Option<usize> {
            self.window
        }
    }

    async fn calls_for(
        messages: &[Message],
        injection_window: usize,
        compact_window: Option<usize>,
        cancel: &CancellationToken,
    ) -> usize {
        let calls = Arc::new(AtomicUsize::new(0));
        let generator = CountingGenerator {
            calls: calls.clone(),
            window: compact_window,
        };
        let _ = compact_messages_with_llm(
            messages,
            None,
            injection_window,
            Some(&generator),
            None,
            None,
            cancel,
            CompactTail::unbounded(),
        )
        .await
        .expect("compact must produce a result");
        calls.load(Ordering::SeqCst)
    }

    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "这是一个用于触发分块压缩的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();
    let compact_window = 64_000;

    let baseline = calls_for(&messages, compact_window, Some(compact_window), &cancel).await;
    let wide_injection = calls_for(&messages, 512_000, Some(compact_window), &cancel).await;
    let narrow_injection = calls_for(&messages, 16_000, Some(compact_window), &cancel).await;
    let wider_compact_model = calls_for(&messages, compact_window, Some(512_000), &cancel).await;

    assert!(
        baseline > 1,
        "测试夹具必须触发 map-reduce 分块，实际调用次数 {baseline}"
    );
    assert_eq!(
        wide_injection, baseline,
        "注入窗口变大不得改变 Map 分块目标"
    );
    assert_eq!(
        narrow_injection, baseline,
        "注入窗口变小不得改变 Map 分块目标"
    );
    assert!(
        wider_compact_model < baseline,
        "compact 模型窗口变大应放宽 Map 分块目标：{wider_compact_model} vs {baseline}"
    );
}

/// summary 预算只跟随注入窗口，与 compact 调用模型窗口无关。
#[tokio::test]
async fn summary_budget_follows_injection_window_not_compact_window() {
    struct BulkyFactsGenerator {
        window: Option<usize>,
    }
    #[async_trait::async_trait]
    impl CompactGenerator for BulkyFactsGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            let text = request
                .first()
                .map(Message::text_content)
                .unwrap_or_default();
            if text.contains("<unprotected_checkpoint_details>") {
                return Ok(CompactGenerationOutputData::from(SHORTER_COMPRESSION_PATCH));
            }
            if text.contains("<compact_facts>") {
                return Ok(CompactGenerationOutputData::from(VALID_CHECKPOINT_WIRE));
            }
            let mut facts = serde_json::json!([
                {
                    "sequence": 1,
                    "source": "main_user",
                    "kind": "constraint",
                    "text": "NEVER widen the requested action level.",
                    "constraint": {
                        "scope": "session",
                        "lifecycle": "persistent",
                        "action": "restrict"
                    }
                },
                {
                    "sequence": 2,
                    "source": "main_user",
                    "kind": "objective",
                    "text": "Continue the compact checkpoint work."
                },
                {
                    "sequence": 3,
                    "source": "main_user",
                    "kind": "resume_candidate",
                    "text": "Validate the generated checkpoint."
                }
            ]);
            for index in 0..12 {
                facts
                    .as_array_mut()
                    .expect("facts array")
                    .push(serde_json::json!({
                        "sequence": index + 10,
                        "source": "tool_result",
                        "kind": "committed_fact",
                        "text": format!("Evidence {index}: {}", "durable observation ".repeat(12))
                    }));
            }
            Ok(CompactGenerationOutputData::from(
                serde_json::json!({ "facts": facts }).to_string(),
            ))
        }

        async fn compact_context_window(&self) -> Option<usize> {
            self.window
        }
    }

    let messages = (0..12)
        .map(|index| Message::user(format!("第 {index} 条需要压缩的历史消息。")))
        .collect::<Vec<_>>();
    let injection_window = 100_000;
    // compact 模型窗口取 10_000（预算 5_000 的十分之一 = 500）：必须显著
    // 小于本用例生成的摘要（~863 tokens），否则断言 `tokens > 该预算` 会因
    // 摘要天然落在预算内而失去判别力（#1773 摘要预留 2%→5% 后 20_000 窗口
    // 的预算已涨到 1_000，恰好盖过摘要）。
    let compact_window = 10_000;
    let result = compact_messages_with_llm(
        &messages,
        None,
        injection_window,
        Some(&BulkyFactsGenerator {
            window: Some(compact_window),
        }),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("compact must produce a result");

    let tokens = crate::domain::token_budget::estimate_tokens(&result.summary);
    assert!(
        tokens <= crate::domain::token_budget::summary_budget(injection_window),
        "summary 必须落在注入窗口预算内，实际 {tokens} tokens"
    );
    assert!(
        tokens > crate::domain::token_budget::summary_budget(compact_window),
        "summary 不得被 compact 模型窗口预算裁剪，实际 {tokens} tokens"
    );
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
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _ = request;
            Ok(CompactGenerationOutputData::from(
                typed_response_for_request(&request),
            ))
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
        None,
        &cancel,
        CompactTail::unbounded(),
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
        None,
        &cancel,
        CompactTail::unbounded(),
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

    let result = compact_messages(&messages, CompactTail::unbounded());

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
fn compact_prompts_require_typed_json_contracts() {
    assert!(COMPACT_PROMPT.contains("Return JSON only"));
    assert!(COMPACT_PROMPT.contains("\"facts\""));
    assert!(COMPACT_PROMPT.contains("scope"));
    assert!(COMPACT_PROMPT.contains("lifecycle"));
    assert!(COMPACT_PROMPT.contains("grant|restrict|revoke|supersede"));
    assert!(COMPACT_PROMPT.contains("\"identity\""));
    assert!(COMPACT_PROMPT.contains("entity + key + dimension"));
    assert!(COMPACT_REFRESH_PROMPT.contains("Return JSON only"));
    assert!(COMPACT_REFRESH_PROMPT.contains("immutable_constraints"));
    assert!(COMPACT_REFRESH_PROMPT.contains("resume_cursor.next_action"));
    // #1623：目标表达与 kind 位置必须显式约束，且修复不得降级 kind 语义。
    assert!(COMPACT_PROMPT.contains("kind=objective"));
    assert!(COMPACT_PROMPT.contains("never downgrade an objective"));
    let repair_request = build_typed_output_repair_request(
        "map",
        "{\"facts\":[]}",
        &crate::domain::CompactGenerationFailureData::new(
            crate::domain::CompactGenerationFailureKind::InvalidSummary,
            "unknown field `resume_candidate`",
        ),
    );
    let repair_text = repair_request
        .first()
        .map(Message::text_content)
        .unwrap_or_default();
    assert!(repair_text.contains("MUST NOT change a fact's kind semantics"));
    for prompt in [COMPACT_PROMPT, COMPACT_REFRESH_PROMPT] {
        assert!(!prompt.contains("<summary>"));
        assert!(!prompt.contains("## Immutable Constraints"));
        assert!(!prompt.contains("Write your summary inside"));
    }
}

/// 用非法探针值触发 serde 枚举错误，从错误信息取回该枚举的合法值全集。
///
/// 测试不重复枚举清单：枚举新增或改名后，serde 的候选值列表自动跟随，
/// prompt 与 schema 的漂移无需人工同步测试断言即可暴露。
fn serde_enum_variants<T: serde::de::DeserializeOwned + std::fmt::Debug>(
    probe_json: &str,
) -> Vec<String> {
    let error = serde_json::from_str::<T>(probe_json).expect_err("探针 JSON 必须被拒绝");
    let message = error.to_string();
    let list = message
        .split_once("expected one of ")
        .unwrap_or_else(|| panic!("探针未产生枚举候选错误：{message}"))
        .1;
    // serde 会在候选清单末尾附加 ` at line L column C` 位置后缀。
    let list = list.split(" at line ").next().unwrap_or(list);
    list.replace('`', "")
        .replace(", or ", ", ")
        .split(",")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect()
}

/// 提取 `"field":"a|b|c"` 形态的 prompt 枚举清单。
fn prompt_pipe_list(source: &str, field: &str) -> Vec<String> {
    let marker = format!("\"{field}\":\"");
    let rest = source
        .split_once(&marker)
        .unwrap_or_else(|| panic!("prompt 缺少 `{field}` 清单"))
        .1;
    let end = rest
        .find('"')
        .unwrap_or_else(|| panic!("`{field}` 清单未闭合"));
    rest[..end].split('|').map(str::to_owned).collect()
}

/// 提取 `Allowed field values: a, b, c.` 形态的 prompt 枚举清单。
fn prompt_allowed_values(source: &str, field: &str) -> Vec<String> {
    let marker = format!("Allowed {field} values: ");
    let rest = source
        .split_once(&marker)
        .unwrap_or_else(|| panic!("prompt 缺少 `{field}` 清单"))
        .1;
    let line = rest.lines().next().unwrap_or_default();
    line.trim_end_matches('.')
        .split(", ")
        .map(str::to_owned)
        .collect()
}

fn constraint_member_json(scope: &str, lifecycle: &str, action: &str) -> String {
    format!(r#"{{"scope":"{scope}","lifecycle":"{lifecycle}","action":"{action}"}}"#)
}

fn identity_member_json(entity: &str, dimension: &str, lifecycle: &str) -> String {
    format!(
        r#"{{"entity":"{entity}","key":"probe-key","dimension":"{dimension}","lifecycle":"{lifecycle}"}}"#
    )
}

/// 端到端：LLM map 提取出的 decision facts 必须渲染进
/// `## Committed Decisions` 分区，与未决决策/风险对照可追溯。
#[tokio::test]
async fn compact_summary_renders_committed_decisions_from_llm_facts() {
    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let generator = MockGenerator {
        text: r#"{"facts":[{"sequence":1,"source":"main_user","kind":"decision","text":"用户拍板引入 ConfirmNode（A 方案）。"},{"sequence":2,"source":"main_user","kind":"objective","text":"继续设计。"},{"sequence":3,"source":"main_user","kind":"resume_candidate","text":"更新文档。"}]}"#.to_string(),
        requests: std::sync::Mutex::new(Vec::new()),
        };

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&generator),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("compact should run");

    assert!(
        result
            .summary
            .contains("## Committed Decisions\n- 用户拍板引入 ConfirmNode（A 方案）。"),
        "决策必须进入独立分区：{}",
        result.summary
    );
}

#[test]
fn compact_request_strips_user_input_timestamp_prefix() {
    // LLM 视图会为真实用户输入渲染时间前缀；摘要请求只吸收对话语义。
    let rendered_view = Message::user("[2026-10-09 15:29 +0800]: 只分析，不实现");
    let request = build_compact_request(&[rendered_view], None, 1_000);
    let prompt = request
        .first()
        .map(Message::text_content)
        .unwrap_or_default();

    assert!(prompt.contains("[User]: 只分析，不实现"));
    assert!(
        !prompt.contains("+0800"),
        "compact 请求不得携带 LLM 视图时间前缀：{prompt}"
    );
}

#[test]
fn compact_request_keeps_plain_user_text_untouched() {
    let request = build_compact_request(&[Message::user("[note]: 参考资料")], None, 1_000);
    let prompt = request
        .first()
        .map(Message::text_content)
        .unwrap_or_default();

    assert!(prompt.contains("[User]: [note]: 参考资料"));
}

#[test]
fn local_fallback_summary_strips_user_input_timestamp_prefix() {
    let messages = vec![Message::user("[2026-10-09 15:29 +0800]: 把当前决策落盘吧")];
    let summary = build_summary_text(&messages, None);

    assert!(summary.contains("把当前决策落盘吧"));
    assert!(
        !summary.contains("+0800"),
        "本地降级摘要不得携带 LLM 视图时间前缀：{summary}"
    );
}

fn checkpoint_for_cascade_probe() -> crate::domain::compact::ContinuationCheckpoint {
    crate::domain::compact::ContinuationCheckpoint::from_sections(
        crate::domain::compact::CheckpointSections {
            immutable_constraints: vec!["- NEVER widen the requested action level.".to_string()],
            current_objective: vec!["- 修复 compact 级联丢失。".to_string()],
            committed_facts: vec!["- CI run 123 已通过。".to_string()],
            uncommitted_working_set: vec!["- 待办：补级联测试。".to_string()],
            open_decisions_and_risks: vec!["- 风险：预算紧张。".to_string()],
            resume_cursor_lines: vec![],
            next_action: "补级联测试".to_string(),
            required_revalidation: vec![],
            committed_decisions: vec!["- 已决策：引入 ConfirmNode（A 方案）。".to_string()],
            archived_milestones: vec![],
            status: crate::domain::compact::ContinuationStatus::Continue,
            status_reason: None,
        },
    )
    .expect("探针 checkpoint 必须可构造")
}

/// compact 级联：上一份 checkpoint 的约束、已提交事实、目标与 resume cursor
/// 经确定性回填后必须原类别存活——不得被降级为 `scope unverified` /
/// `unverified fact` 风险，也不得被静默丢弃。
#[test]
fn checkpoint_cascade_keeps_previous_sections_in_place() {
    let previous = checkpoint_for_cascade_probe();
    let wire_before = previous.to_wire();
    let batch = checkpoint_to_fact_batch(previous);
    let reduced = crate::domain::compact::reduce_compact_facts(batch).expect("级联归并必须成功");
    let wire_after = reduced.to_wire();

    assert!(
        wire_after
            .immutable_constraints
            .iter()
            .any(|line| line.contains("NEVER widen the requested action level.")),
        "上一份 checkpoint 的不可变约束在级联后丢失：{:?}",
        wire_after.immutable_constraints
    );
    assert!(
        wire_after
            .committed_facts
            .iter()
            .any(|line| line.contains("CI run 123 已通过。")),
        "上一份 checkpoint 的已提交事实被降级或丢弃：{:?}",
        wire_after
    );
    assert!(
        wire_after
            .open_decisions_and_risks
            .iter()
            .all(|line| !line.contains("unverified") && !line.contains("scope unverified")),
        "级联不得产生 unverified / scope unverified 降级：{:?}",
        wire_after.open_decisions_and_risks
    );
    assert!(
        wire_after
            .current_objective
            .contains("修复 compact 级联丢失"),
        "上一份 checkpoint 的目标被丢弃：{}",
        wire_after.current_objective
    );
    assert!(
        wire_after.resume_cursor.next_action.contains("补级联测试"),
        "上一份 checkpoint 的 resume cursor 被丢弃：{}",
        wire_after.resume_cursor.next_action
    );
    assert!(
        wire_after
            .uncommitted_working_set
            .iter()
            .any(|line| line.contains("待办：补级联测试。")),
        "上一份 checkpoint 的 working set 丢失：{:?}",
        wire_after.uncommitted_working_set
    );
    assert!(
        wire_after
            .committed_decisions
            .iter()
            .any(|line| line.contains("引入 ConfirmNode（A 方案）")),
        "上一份 checkpoint 的已决策项被丢弃或降级：{:?}",
        wire_after.committed_decisions
    );
    assert!(
        wire_after
            .open_decisions_and_risks
            .iter()
            .all(|line| !line.contains("引入 ConfirmNode")),
        "已决策项不得降级进未决区：{:?}",
        wire_after.open_decisions_and_risks
    );
    assert_eq!(
        wire_before.immutable_constraints.len(),
        wire_after.immutable_constraints.len(),
        "约束条目数在级联后发生变化"
    );
}

/// map 阶段单个 chunk 的 Provider 级失败必须本地降级，而不是作废整个
/// map-reduce：已完成 chunk 的 LLM facts 必须保留。历史缺陷——任一 chunk
/// 报错会让全部 chunk 退化为本地文本降级，已提取的事实随截断丢失。
#[tokio::test]
async fn map_chunk_provider_failure_degrades_locally_and_keeps_other_chunks() {
    struct FlakyChunkGenerator;
    #[async_trait::async_trait]
    impl CompactGenerator for FlakyChunkGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            let prompt = request
                .first()
                .map(Message::text_content)
                .unwrap_or_default();
            if prompt.contains("POISON-CHUNK") {
                return Err(crate::domain::CompactGenerationFailureData::new(
                    crate::domain::CompactGenerationFailureKind::Provider,
                    "模拟 chunk 级 Provider 失败",
                ));
            }
            let text = if prompt.contains("CHUNK-KEEP-MARKER") {
                "保留的 chunk 事实"
            } else {
                "其他 chunk 事实"
            };
            Ok(CompactGenerationOutputData::from(format!(
                r#"{{"facts":[{{"sequence":1,"source":"main_user","kind":"objective","text":"{text}"}},{{"sequence":2,"source":"main_user","kind":"resume_candidate","text":"继续"}}]}}"#
            )))
        }
    }

    let messages = (0..600)
        .map(|index| {
        let marker = match index {
        10 => " POISON-CHUNK ",
        500 => " CHUNK-KEEP-MARKER ",
        _ => " ",
        };
        Message::user(format!(
        "编号 {index}。{marker}需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。{}",
        "重复填充内容以放大估算。".repeat(3)
        ))
        })
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&FlakyChunkGenerator),
        None,
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await
    .expect("chunk 级失败不得让整次 compact 失败");

    assert!(
        result.summary.contains("保留的 chunk 事实"),
        "其他 chunk 的 LLM facts 被整体作废：{}",
        result.summary
    );
    assert!(
        matches!(
        result.quality,
        crate::domain::CompactSummaryQuality::PartialMapFallback { degraded_chunks, .. }
        if degraded_chunks >= 1
        ),
        "必须记录 chunk 级降级，实际 {:?}",
        result.quality
    );
}

/// 本地降级的截断上限必须足以保住决策原文：历史缺陷为每条消息 200 字符、
/// 每条工具结果 500 字符，会在决策句中途截断。上限放大后由注入侧的 summary
/// 预算统一收敛，不会撑爆上下文。
#[test]
fn local_fallback_keeps_long_user_deliberation_beyond_legacy_cap() {
    // 旧上限 200 字节 ≈ 66 个汉字，会在决策句中途截断。
    let decision_text = "决定：Replan 只能修改 Pending 节点，Active 节点冻结。".repeat(60);
    let beyond_legacy = decision_text.chars().take(300).collect::<String>();
    // 1500 个字符 ≈ 2650 字节，超过 2000 字节上限。
    let beyond_cap = decision_text.chars().take(1_500).collect::<String>();
    let summary = build_summary_text(&[Message::user(decision_text)], None);

    assert!(
        summary.contains(&beyond_legacy),
        "提升后的上限应保留至少 300 个字符的决策原文，实际 summary 字节数 {}",
        summary.len()
    );
    assert!(!summary.contains(&beyond_cap), "超过上限的内容必须被截断");
}

#[test]
fn local_fallback_keeps_long_tool_result_beyond_legacy_cap() {
    let result_text = "cargo test -p context: 513 passed; 2222".repeat(200);
    let beyond_legacy = result_text.chars().take(600).collect::<String>();
    let beyond_cap = result_text.chars().take(4_500).collect::<String>();
    let tool_result = Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "call-long".to_string(),
            content: serde_json::Value::String(result_text.clone()),
            text: Some(result_text),
            is_error: false,
        }],
        metadata: None,
    };

    let summary = build_summary_text(&[tool_result], None);

    assert!(
        summary.contains(&beyond_legacy),
        "提升后的上限应保留至少 600 个字符的工具结果，实际 summary 字节数 {}",
        summary.len()
    );
    assert!(
        !summary.contains(&beyond_cap),
        "超过上限的工具结果必须被截断"
    );
}

/// prompt 声明的枚举字面量必须与 serde 枚举双向一致：既不出现 serde 拒绝的
/// 值（历史缺陷：prompt 写 `task`、枚举是 `task_data`，导致 map chunk 降级），
/// 也不遗漏枚举已有的变体。
#[test]
fn compact_prompt_enum_literals_match_serde_enums() {
    use crate::domain::compact as compact_domain;

    fn scoped<'a>(source: &'a str, marker: &'a str) -> &'a str {
        source
            .split_once(marker)
            .map(|(_, tail)| tail)
            .unwrap_or_else(|| panic!("prompt 缺少 `{marker}` 段"))
    }

    let prompt = COMPACT_PROMPT;
    let constraint_section = scoped(prompt, "\"constraint\"");
    let identity_section = scoped(prompt, "\"identity\"");

    /// 单条 prompt↔serde 枚举对账契约。
    struct EnumContractCheck {
        label: &'static str,
        prompt_values: Vec<String>,
        serde_values: Vec<String>,
        accepts: Box<dyn Fn(&str) -> bool>,
        /// 内部值豁免：由代码路径写入、绝不暴露给 LLM 的 serde 枚举值
        /// （新增变体时必须显式决定是否列入，列入即声明 LLM 看不到它）。
        internal_values: &'static [&'static str],
    }

    let checks: Vec<EnumContractCheck> = vec![
        EnumContractCheck {
            label: "constraint.scope",
            prompt_values: prompt_pipe_list(constraint_section, "scope"),
            serde_values: serde_enum_variants::<compact_domain::ConstraintMetadata>(
                &constraint_member_json("__probe__", "persistent", "grant"),
            ),
            accepts: Box::new(|value| {
                serde_json::from_str::<compact_domain::ConstraintMetadata>(&constraint_member_json(
                    value,
                    "persistent",
                    "grant",
                ))
                .is_ok()
            }),
            internal_values: &[],
        },
        EnumContractCheck {
            label: "constraint.lifecycle",
            prompt_values: prompt_pipe_list(constraint_section, "lifecycle"),
            serde_values: serde_enum_variants::<compact_domain::ConstraintMetadata>(
                &constraint_member_json("session", "__probe__", "grant"),
            ),
            accepts: Box::new(|value| {
                serde_json::from_str::<compact_domain::ConstraintMetadata>(&constraint_member_json(
                    "session", value, "grant",
                ))
                .is_ok()
            }),
            internal_values: &[],
        },
        EnumContractCheck {
            label: "constraint.action",
            prompt_values: prompt_pipe_list(constraint_section, "action"),
            serde_values: serde_enum_variants::<compact_domain::ConstraintMetadata>(
                &constraint_member_json("session", "persistent", "__probe__"),
            ),
            accepts: Box::new(|value| {
                serde_json::from_str::<compact_domain::ConstraintMetadata>(&constraint_member_json(
                    "session",
                    "persistent",
                    value,
                ))
                .is_ok()
            }),
            internal_values: &[],
        },
        EnumContractCheck {
            label: "identity.entity",
            prompt_values: prompt_pipe_list(identity_section, "entity"),
            serde_values: serde_enum_variants::<compact_domain::CompactFactIdentity>(
                &identity_member_json("__probe__", "status", "dynamic"),
            ),
            accepts: Box::new(|value| {
                serde_json::from_str::<compact_domain::CompactFactIdentity>(&identity_member_json(
                    value, "status", "dynamic",
                ))
                .is_ok()
            }),
            internal_values: &[],
        },
        EnumContractCheck {
            label: "identity.dimension",
            prompt_values: prompt_pipe_list(identity_section, "dimension"),
            serde_values: serde_enum_variants::<compact_domain::CompactFactIdentity>(
                &identity_member_json("branch", "__probe__", "dynamic"),
            ),
            accepts: Box::new(|value| {
                serde_json::from_str::<compact_domain::CompactFactIdentity>(&identity_member_json(
                    "branch", value, "dynamic",
                ))
                .is_ok()
            }),
            internal_values: &[],
        },
        EnumContractCheck {
            label: "identity.lifecycle",
            prompt_values: prompt_pipe_list(identity_section, "lifecycle"),
            serde_values: serde_enum_variants::<compact_domain::CompactFactIdentity>(
                &identity_member_json("branch", "status", "__probe__"),
            ),
            accepts: Box::new(|value| {
                serde_json::from_str::<compact_domain::CompactFactIdentity>(&identity_member_json(
                    "branch", "status", value,
                ))
                .is_ok()
            }),
            internal_values: &[],
        },
        EnumContractCheck {
            label: "fact.source",
            prompt_values: prompt_allowed_values(prompt, "source"),
            serde_values: serde_enum_variants::<compact_domain::CompactFactSource>("\"__probe__\""),
            accepts: Box::new(|value| {
                serde_json::from_str::<compact_domain::CompactFactSource>(&format!("\"{value}\""))
                    .is_ok()
            }),
            internal_values: &["checkpoint"],
        },
        EnumContractCheck {
            label: "fact.kind",
            prompt_values: prompt_allowed_values(prompt, "kind"),
            serde_values: serde_enum_variants::<compact_domain::CompactFactKind>("\"__probe__\""),
            accepts: Box::new(|value| {
                serde_json::from_str::<compact_domain::CompactFactKind>(&format!("\"{value}\""))
                    .is_ok()
            }),
            internal_values: &[],
        },
    ];

    for check in checks {
        let label = check.label;
        assert!(
            !check.prompt_values.is_empty(),
            "{label}：prompt 清单解析为空"
        );
        for value in &check.prompt_values {
            assert!(
                (check.accepts)(value),
                "{label}：prompt 声明 `{value}`，serde 枚举不接受该值"
            );
        }
        for value in &check.serde_values {
            let declared = check.prompt_values.contains(value)
                || check.internal_values.contains(&value.as_str());
            assert!(
                declared,
                "{label}：serde 枚举存在 `{value}`，prompt 未声明且不在内部豁免清单"
            );
        }
        for value in check.internal_values {
            assert!(
                !check.prompt_values.iter().any(|declared| declared == value),
                "{label}：内部值 `{value}` 不得出现在 prompt 清单中"
            );
        }
    }
}

#[test]
fn typed_checkpoint_wire_is_normalized_before_commit() {
    let wire: crate::domain::compact::ContinuationCheckpointWire =
        serde_json::from_str(VALID_CHECKPOINT_WIRE).unwrap();
    let checkpoint = crate::domain::compact::ContinuationCheckpoint::try_from(wire)
        .unwrap()
        .normalize_to_budget(10_000)
        .unwrap();
    let rendered = checkpoint.render();

    assert_eq!(
        crate::domain::compact::ContinuationCheckpoint::parse(&rendered).unwrap(),
        checkpoint
    );
}

#[test]
fn typed_checkpoint_wire_rejects_invalid_schema() {
    let error = decode_typed_json::<crate::domain::compact::ContinuationCheckpointWire>(
        "reduce",
        r#"{"current_objective":"legacy","unexpected":true}"#,
    )
    .expect_err("invalid typed schema must fail");

    assert_eq!(
        error.kind,
        crate::domain::CompactGenerationFailureKind::InvalidSummary
    );
    assert!(error.message.contains("reduce compact JSON 无效"));
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
        text.matches("extracting continuation-critical facts")
            .count(),
        1
    );
    assert_eq!(text.matches("<conversation_history>").count(), 1);
    assert!(text.contains("<previous_checkpoint>"));
    assert!(text.contains("unverified legacy summary"));
    assert!(text.contains("继续检查 compact"));
}

/// LLM 压缩请求的 previous checkpoint 必须按语义预算有界化，
/// 避免上次 summary 巨大时压缩请求超 provider 输入限制，同时不依赖尾部位置保真。
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
        text.contains("<previous_checkpoint>"),
        "应使用 checkpoint 标记: {text}"
    );
    assert!(!text.contains("<previous_summary_tail>"));
    assert!(!text.contains("older head truncated"));
}

#[test]
fn fallback_subagent_read_only_instruction_does_not_become_session_constraint() {
    let summary = build_summary_text(
        &[
            Message::user("Agent prompt: only investigate; do not edit files."),
            Message::user("Implement the root-cause fix."),
        ],
        None,
    );

    let immutable = summary
        .split("## Immutable Constraints\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Current Objective")
        .next()
        .unwrap();
    let objective = summary
        .split("## Current Objective\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Committed Facts")
        .next()
        .unwrap();

    assert!(!immutable.contains("do not edit files"));
    assert!(!immutable.contains("do not infer new authority"));
    assert!(objective.contains("Implement the root-cause fix"));
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

    assert!(summary.contains("## Current Objective"));
    assert!(summary.contains("只分析，不实现"));
    assert!(summary.contains("## Committed Facts"));
    assert!(summary.contains("Unverified assistant report"));
    assert!(!summary.contains("- 已完成"));
    assert!(summary.contains("## Open Decisions / Risks"));
    assert!(summary.contains("## Resume Cursor"));
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
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await
    .expect("second compact should run");

    assert!(
        result.summary.contains("unverified legacy summary")
            && result
                .summary
                .contains("first compact summary with original user request"),
        "second compact must conservatively retain legacy previous summary: {}",
        result.summary
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
        None,
        &cancel,
        CompactTail::unbounded(),
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

/// fallback 对 oversized previous checkpoint 按语义收敛，不依赖尾部截断。
#[test]
fn fallback_normalizes_oversized_previous_checkpoint() {
    let previous = oversized_valid_checkpoint(800_000);

    let summary = crate::adapters::compact_summary::build_summary_text(&[], Some(&previous));

    assert!(summary.contains("NEVER widen the requested action level"));
    assert!(!summary.contains("archive-noise archive-noise archive-noise"));
    let cap = crate::adapters::compact_summary::FALLBACK_PREVIOUS_SUMMARY_CAP;
    assert!(summary.len() <= cap + 2_000);
    crate::domain::compact::ContinuationCheckpoint::parse(&summary).unwrap();
}

#[tokio::test]
async fn multi_chunk_map_is_reduced_locally_without_full_checkpoint_request() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "触发本地归并的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let map_calls = Arc::new(AtomicUsize::new(0));
    let forbidden_full_checkpoint_calls = Arc::new(AtomicUsize::new(0));

    struct LocalReduceGenerator {
        map_calls: Arc<AtomicUsize>,
        forbidden_full_checkpoint_calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl CompactGenerator for LocalReduceGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            let prompt = request
                .first()
                .map(Message::text_content)
                .unwrap_or_default();
            if prompt.contains("<compact_facts>") {
                self.forbidden_full_checkpoint_calls
                    .fetch_add(1, Ordering::SeqCst);
                return Ok(CompactGenerationOutputData::from(VALID_CHECKPOINT_WIRE));
            }
            self.map_calls.fetch_add(1, Ordering::SeqCst);
            Ok(CompactGenerationOutputData::from(VALID_MAP_FACTS))
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&LocalReduceGenerator {
            map_calls: map_calls.clone(),
            forbidden_full_checkpoint_calls: forbidden_full_checkpoint_calls.clone(),
        }),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("multi-chunk facts should be reduced locally");

    assert!(map_calls.load(Ordering::SeqCst) >= 3);
    assert_eq!(forbidden_full_checkpoint_calls.load(Ordering::SeqCst), 0);
    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(&result.summary)
        .expect("local reducer must render a valid checkpoint");
    assert_eq!(checkpoint.resume_cursor().next_action_count(), 1);
    assert!(result
        .summary
        .contains("Continue the compact checkpoint work"));
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
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
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
            let _ = text;
            Ok(CompactGenerationOutputData::from(
                typed_response_for_request(&request),
            ))
        }
    }

    let generator = ObservingGenerator {
        current,
        max_concurrent,
        call_count,
    };

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&generator),
        None,
        None,
        &cancel,
        CompactTail::unbounded(),
    )
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
    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(&result.summary)
        .expect("map-reduce must render a valid typed checkpoint");
    assert_eq!(
        checkpoint.status(),
        crate::domain::compact::ContinuationStatus::Continue
    );
    assert_eq!(checkpoint.resume_cursor().next_action_count(), 1);
    assert!(result
        .summary
        .contains("## Current Objective\n- Continue the compact checkpoint work."));
}

/// Rust-owned Reduce must normalize an oversized result within budget without a full-checkpoint LLM request.
#[tokio::test]
async fn local_reduce_normalizes_oversized_unprotected_facts_to_budget() {
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    /// Map returns an oversized milestone fact; Refresh returns a bounded patch.
    struct ShrinkingGenerator {
        call_count: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl CompactGenerator for ShrinkingGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let text = request
                .first()
                .map(|msg| msg.text_content())
                .unwrap_or_default();
            if text.contains("<unprotected_checkpoint_details>") {
                Ok(CompactGenerationOutputData::from(SHORTER_COMPRESSION_PATCH))
            } else {
                let mut batch: serde_json::Value = serde_json::from_str(VALID_MAP_FACTS).unwrap();
                batch["facts"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!({
                        "sequence": 4,
                        "source": "tool_result",
                        "kind": "milestone",
                        "text": format!("Archive abc: {}", "historical detail ".repeat(80_000))
                    }));
                Ok(CompactGenerationOutputData::from(batch.to_string()))
            }
        }
    }

    let generator = ShrinkingGenerator { call_count };

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&generator),
        None,
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await
    .expect("compact should run");

    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(&result.summary)
        .expect("refresh must leave a valid typed checkpoint");
    assert_eq!(checkpoint.resume_cursor().next_action_count(), 1);
    assert!(!result.summary.contains("historical detail"));
    assert_eq!(
        generator.call_count.load(Ordering::SeqCst),
        5,
        "deterministic normalization should finish without an LLM Refresh"
    );
}

/// Generator that returns a canned typed map response regardless of input.
struct MockGenerator {
    text: String,
    requests: std::sync::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl CompactGenerator for MockGenerator {
    async fn generate(
        &self,
        request: Vec<Message>,
        _cancel: &CancellationToken,
    ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData> {
        self.requests.lock().unwrap().push(
            request
                .first()
                .map(Message::text_content)
                .unwrap_or_default(),
        );
        Ok(CompactGenerationOutputData::from(self.text.clone()))
    }
}

#[tokio::test]
async fn compact_with_generator_uses_llm_summary() {
    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    let generator = MockGenerator {
        text: VALID_MAP_FACTS.to_string(),
        requests: std::sync::Mutex::new(Vec::new()),
    };

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&generator),
        None,
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await
    .expect("compact should run");

    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(&result.summary)
        .expect("typed facts must be rendered by the local checkpoint renderer");
    assert_eq!(checkpoint.resume_cursor().next_action_count(), 1);
    assert!(result
        .summary
        .contains("## Current Objective\n- Continue the compact checkpoint work."));
    assert_eq!(result.quality, crate::domain::CompactSummaryQuality::Llm);
    assert!(!result.summary.contains("Local text-compaction path"));

    let request = generator.requests.lock().unwrap().join("\n");
    assert!(request.contains("JSON only"));
    assert!(request.contains("\"facts\""));
    assert!(!request.contains("<summary>"));
    assert!(!request.contains("## Immutable Constraints"));
}

#[tokio::test]
async fn compact_cancelled_generator_does_not_fallback() {
    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();
    cancel.cancel();

    struct CancelledGenerator;
    #[async_trait::async_trait]
    impl CompactGenerator for CancelledGenerator {
        async fn generate(
            &self,
            _request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            Err(crate::domain::CompactGenerationFailureData::new(
                crate::domain::CompactGenerationFailureKind::Cancelled,
                "cancelled",
            ))
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&CancelledGenerator),
        None,
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await;

    assert!(
        result.is_none(),
        "cancelled compact must not create fallback"
    );
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
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            Err(crate::domain::CompactGenerationFailureData::new(
                crate::domain::CompactGenerationFailureKind::Provider,
                "simulated LLM failure",
            ))
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&FailingGenerator),
        None,
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await
    .expect("compact should still run with fallback");

    // Fallback summary 使用本地 checkpoint 模板。
    assert!(result.summary.contains("## Current Objective"));
    assert!(result.summary.contains("Local text-compaction path"));
    assert!(!result.summary.contains("Semantic LLM compaction failed"));
    assert_eq!(
        result.quality,
        crate::domain::CompactSummaryQuality::LocalFallback(
            crate::domain::CompactGenerationFailureKind::Provider
        )
    );
}

/// #1490：再压提示词必须使用缩减预算（summary_budget × 0.8）并硬约束，
/// 替代通用 COMPACT_PROMPT 的 "more detail is better" 反效果措辞。
/// 再压（refresh）MUST NOT 把决策/风险区当作可变字段下发：该区由用户拍板
/// 与未决项构成，LLM 只能压缩非决策区（历史缺陷：提示词要求 aggressively
/// drop，决策随再压被删）。
#[test]
fn refresh_request_excludes_open_decisions_from_mutable_patch() {
    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(VALID_CHECKPOINT)
        .expect("fixture must be valid");
    let prompt = crate::adapters::compact_summary::build_refresh_prompt(&checkpoint, 5_000);

    assert!(
        !prompt.contains("\"open_decisions_and_risks\":"),
        "再压请求不得把决策区作为可变字段下发：{prompt}"
    );
    assert!(
        prompt.contains("open_decisions_and_risks"),
        "prompt 必须声明决策区受保护、不可改写：{prompt}"
    );
}

#[test]
fn refresh_prompt_enforces_shrunk_budget() {
    use crate::domain::token_budget::summary_budget;

    let budget = summary_budget(272_000); // 5440
    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(VALID_CHECKPOINT)
        .expect("fixture must be valid");
    let prompt = crate::adapters::compact_summary::build_refresh_prompt(&checkpoint, budget);

    assert!(
        prompt.contains("MUST help the rendered checkpoint fit within"),
        "再压提示词必须有硬预算约束: {prompt}"
    );
    let expected_prompt_budget = budget * 8 / 10; // ×0.8 留余量
    assert!(
        prompt.contains(&expected_prompt_budget.to_string()),
        "提示预算应为 summary_budget×0.8（{expected_prompt_budget}）: {prompt}"
    );
    assert!(prompt.contains("JSON only"));
    assert!(prompt.contains("<unprotected_checkpoint_details>"));
    assert!(prompt.contains("\"resume_context\""));
    assert!(!prompt.contains("\"immutable_constraints\""));
    assert!(!prompt.contains("## Immutable Constraints"));
    assert!(!prompt.contains("<summary>"));
}

/// Deterministic normalization removes oversized duplicate details before Refresh.
#[tokio::test]
async fn local_reduce_normalization_avoids_non_shrinking_refresh_rounds() {
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    let calls = Arc::new(AtomicUsize::new(0));

    /// Map returns oversized facts; deterministic normalization prevents Refresh.
    struct NeverShrinkingGenerator {
        calls: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl CompactGenerator for NeverShrinkingGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let text = request
                .first()
                .map(|msg| msg.text_content())
                .unwrap_or_default();
            if text.contains("<unprotected_checkpoint_details>") {
                return Ok(CompactGenerationOutputData::from(
                    r#"{
                  "committed_facts": [],
                  "uncommitted_working_set": [],
                  "resume_context": [],
                  "required_revalidation": [],
                  "archived_milestones": ["historical detail"]
                }"#,
                ));
            }
            let mut batch: serde_json::Value = serde_json::from_str(VALID_MAP_FACTS).unwrap();
            batch["facts"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "sequence": 4,
                    "source": "tool_result",
                    "kind": "milestone",
                    "text": format!("Archive abc: {}", "historical detail ".repeat(40_000))
                }));
            Ok(CompactGenerationOutputData::from(batch.to_string()))
        }
    }

    let generator = NeverShrinkingGenerator {
        calls: calls.clone(),
    };
    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&generator),
        None,
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await
    .expect("compact should run");

    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(&result.summary)
        .expect("local normalization must leave a valid typed checkpoint");
    assert_eq!(checkpoint.resume_cursor().next_action_count(), 1);
    assert!(result.summary.contains("historical detail"));
    assert_eq!(calls.load(Ordering::SeqCst), 5);
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
    let seen = Arc::new(Mutex::new(Vec::<(String, CompactWorkData)>::new()));

    struct EchoGenerator;
    #[async_trait::async_trait]
    impl CompactGenerator for EchoGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            Ok(CompactGenerationOutputData::from(
                typed_response_for_request(&request),
            ))
        }
    }

    let progress = {
        let seen = seen.clone();
        move |stage: CompactStageData, work: CompactWorkData| {
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
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await
    .expect("compact should run");
    crate::domain::compact::ContinuationCheckpoint::parse(&result.summary)
        .expect("progress path must render a valid typed checkpoint");

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
    let CompactWorkData::Determinate {
        completed: first_completed,
        total: first_total,
    } = mapping[0].1
    else {
        panic!("mapping 必须是 determinate work")
    };
    let CompactWorkData::Determinate {
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
    let seen = Arc::new(Mutex::new(Vec::<(String, CompactWorkData)>::new()));

    struct EchoGenerator;
    #[async_trait::async_trait]
    impl CompactGenerator for EchoGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            Ok(CompactGenerationOutputData::from(
                typed_response_for_request(&request),
            ))
        }
    }

    let progress = {
        let seen = seen.clone();
        move |stage: CompactStageData, work: CompactWorkData| {
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
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await
    .expect("compact should run");
    crate::domain::compact::ContinuationCheckpoint::parse(&result.summary)
        .expect("progress path must render a valid typed checkpoint");

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
            .all(|(_, work)| *work == CompactWorkData::Indeterminate),
        "单次摘要不应伪造 chunk 计数，实际 {seen:?}"
    );
}

#[tokio::test]
async fn empty_single_map_retries_once_with_original_history() {
    use std::sync::Mutex;

    let messages = (0..10)
        .map(|index| Message::user(format!("original-message-{index}")))
        .collect::<Vec<_>>();
    let requests = Arc::new(Mutex::new(Vec::new()));

    struct EmptyThenValidGenerator {
        requests: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl CompactGenerator for EmptyThenValidGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            let prompt = request
                .iter()
                .map(Message::text_content)
                .collect::<Vec<_>>()
                .join("\n");
            let mut requests = self.requests.lock().unwrap();
            requests.push(prompt);
            if requests.len() == 1 {
                Ok(CompactGenerationOutputData::completed(
                    "",
                    Some("end_turn"),
                    0,
                    1,
                ))
            } else {
                Ok(CompactGenerationOutputData::completed(
                    VALID_MAP_FACTS,
                    Some("end_turn"),
                    1,
                    0,
                ))
            }
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&EmptyThenValidGenerator {
            requests: requests.clone(),
        }),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("empty map should retry with original history");

    assert_eq!(result.quality, crate::domain::CompactSummaryQuality::Llm);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("original-message-0"));
    assert!(requests[1].contains("original-message-0"));
    assert!(requests[1].contains("previous attempt returned no text"));
}

#[tokio::test]
async fn one_persistently_empty_map_chunk_degrades_locally_without_losing_other_facts() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "chunk-marker-{index} {}",
                "long compact history ".repeat(80)
            ))
        })
        .collect::<Vec<_>>();
    let empty_calls = Arc::new(AtomicUsize::new(0));

    struct PartiallyEmptyGenerator {
        empty_calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl CompactGenerator for PartiallyEmptyGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            let prompt = request
                .iter()
                .map(Message::text_content)
                .collect::<Vec<_>>()
                .join("\n");
            if prompt.contains("chunk-marker-0") {
                self.empty_calls.fetch_add(1, Ordering::SeqCst);
                return Ok(CompactGenerationOutputData::completed(
                    "",
                    Some("end_turn"),
                    0,
                    1,
                ));
            }
            Ok(CompactGenerationOutputData::completed(
                r#"{"facts":[{"sequence":700,"source":"tool_result","kind":"committed_fact","text":"successful chunk fact survived"},{"sequence":701,"source":"main_user","kind":"objective","text":"Continue chunk recovery."},{"sequence":702,"source":"main_user","kind":"resume_candidate","text":"Verify partial degradation."}]}"#,
                Some("end_turn"),
                1,
                0,
            ))
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&PartiallyEmptyGenerator {
            empty_calls: empty_calls.clone(),
        }),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("one failed chunk should not discard successful facts");

    assert_eq!(empty_calls.load(Ordering::SeqCst), 2);
    assert!(matches!(
        result.quality,
        crate::domain::CompactSummaryQuality::PartialMapFallback {
            degraded_chunks: 1,
            failure: crate::domain::CompactGenerationFailureKind::InvalidSummary
        }
    ));
    assert!(result.summary.contains("successful chunk fact survived"));
    assert!(result.summary.contains("chunk-marker-0"));
    assert_eq!(result.summary.matches("- Next action:").count(), 1);
}

#[tokio::test]
async fn previous_summary_with_task_companion_reaches_typed_reduce() {
    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "continued-compact-{index} {}",
                "history ".repeat(100)
            ))
        })
        .collect::<Vec<_>>();

    struct SuccessfulMapGenerator;

    #[async_trait::async_trait]
    impl CompactGenerator for SuccessfulMapGenerator {
        async fn generate(
            &self,
            _request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            Ok(CompactGenerationOutputData::from(VALID_MAP_FACTS))
        }
    }

    let previous = format!(
        "{VALID_CHECKPOINT}\n\n## Current Task State\nBatch #13 — Tasks: 1/7\n■ [task:2] 定义 Execution Specifications"
    );
    let result = compact_messages_with_llm(
        &messages,
        Some(&previous),
        100_000,
        Some(&SuccessfulMapGenerator),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("task companion must not invalidate previous checkpoint");

    assert_eq!(result.quality, crate::domain::CompactSummaryQuality::Llm);
    assert!(result
        .summary
        .contains("Existing compact tests passed before this change."));
    assert!(!result.summary.contains("Current Task State"));
    assert_eq!(result.summary.matches("- Next action:").count(), 1);
}

#[tokio::test]
async fn partial_map_fallback_preserves_previous_constraints() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "protected-chunk-{index} {}",
                "history ".repeat(100)
            ))
        })
        .collect::<Vec<_>>();
    let first_chunk_calls = Arc::new(AtomicUsize::new(0));

    struct EmptyFirstChunkGenerator {
        first_chunk_calls: Arc<AtomicUsize>,
        previous_facts: String,
    }

    #[async_trait::async_trait]
    impl CompactGenerator for EmptyFirstChunkGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            let prompt = request
                .iter()
                .map(Message::text_content)
                .collect::<Vec<_>>()
                .join("\n");
            if prompt.contains("protected-chunk-0")
                && !prompt.contains("\"kind\":\"working_item\"")
                && !prompt.contains("## Committed Facts")
            {
                self.first_chunk_calls.fetch_add(1, Ordering::SeqCst);
                return Ok(CompactGenerationOutputData::completed(
                    "",
                    Some("end_turn"),
                    0,
                    0,
                ));
            }
            if prompt.contains("<previous_checkpoint>") {
                return Ok(CompactGenerationOutputData::from(
                    self.previous_facts.clone(),
                ));
            }
            Ok(CompactGenerationOutputData::from(VALID_MAP_FACTS))
        }
    }

    let previous = VALID_CHECKPOINT.replacen(
        "- NEVER widen the requested action level.",
        "- NEVER remove the protected previous constraint.",
        1,
    );
    let result = compact_messages_with_llm(
        &messages,
        Some(&previous),
        100_000,
        Some(&EmptyFirstChunkGenerator {
            first_chunk_calls: first_chunk_calls.clone(),
            previous_facts: r#"{"facts":[{"kind":"constraint","text":"NEVER remove the protected previous constraint.","provenance":{"source":"main_user","order":0},"authority":"main_user","scope":"session","lifecycle":"persistent","action":"never","entity":null,"key":null,"dimension":null}]}"#.to_string(),
        }),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("partial fallback must preserve previous protected semantics");

    assert!(result
        .summary
        .contains("NEVER widen the requested action level."));
    assert!(first_chunk_calls.load(Ordering::SeqCst) <= 2);
}

#[tokio::test]
async fn invalid_single_map_json_is_repaired_before_fallback() {
    use std::sync::Mutex;

    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let requests = Arc::new(Mutex::new(Vec::new()));

    struct RepairingMapGenerator {
        requests: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl CompactGenerator for RepairingMapGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            let prompt = request
                .first()
                .map(Message::text_content)
                .unwrap_or_default();
            let mut requests = self.requests.lock().unwrap();
            requests.push(prompt.clone());
            if requests.len() == 1 {
                Ok(CompactGenerationOutputData::from(
                    r#"{"facts":"not-an-array"}"#,
                ))
            } else {
                Ok(CompactGenerationOutputData::from(VALID_MAP_FACTS))
            }
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&RepairingMapGenerator {
            requests: requests.clone(),
        }),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("repair should preserve compact");

    assert_eq!(result.quality, crate::domain::CompactSummaryQuality::Llm);
    assert!(!result.summary.contains("Local text-compaction path"));
    let requests = requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "invalid map should receive one repair call"
    );
    assert!(requests[1].contains("map"));
    assert!(requests[1].contains("invalid type"));
    assert!(requests[1].contains(r#"{"facts":"not-an-array"}"#));
}

#[tokio::test]
async fn local_reduce_never_repairs_a_full_checkpoint_wire() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "触发分块压缩的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let full_checkpoint_calls = Arc::new(AtomicUsize::new(0));

    struct MapOnlyGenerator(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl CompactGenerator for MapOnlyGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            let prompt = request
                .first()
                .map(Message::text_content)
                .unwrap_or_default();
            if prompt.contains("<compact_facts>") || prompt.contains("repairing the reduce") {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
            Ok(CompactGenerationOutputData::from(VALID_MAP_FACTS))
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&MapOnlyGenerator(full_checkpoint_calls.clone())),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("local reduce should preserve compact");

    assert_eq!(full_checkpoint_calls.load(Ordering::SeqCst), 0);
    assert_eq!(result.quality, crate::domain::CompactSummaryQuality::Llm);
    assert!(result
        .summary
        .contains("Continue the compact checkpoint work"));
}

#[tokio::test]
async fn refresh_repair_provider_failure_degrades_to_bounded_canonical_checkpoint() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "触发分块压缩的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let calls = Arc::new(AtomicUsize::new(0));

    struct InvalidRefreshThenUnavailable {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl CompactGenerator for InvalidRefreshThenUnavailable {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let prompt = request
                .first()
                .map(Message::text_content)
                .unwrap_or_default();
            if prompt.contains("repairing the refresh") {
                return Err(crate::domain::CompactGenerationFailureData::new(
                    crate::domain::CompactGenerationFailureKind::Provider,
                    "provider upstream unavailable",
                ));
            }
            if prompt.contains("<unprotected_checkpoint_details>") {
                return Ok(CompactGenerationOutputData::from(
                    r#"{"committed_facts":["unterminated""#,
                ));
            }
            let mut batch: serde_json::Value = serde_json::from_str(VALID_MAP_FACTS).unwrap();
            let facts = batch["facts"].as_array_mut().unwrap();
            for fact_index in 0..240 {
                facts.push(serde_json::json!({
                    "sequence": fact_index + 10,
                    "source": "tool_result",
                    "kind": "committed_fact",
                    "text": format!(
                        "Evidence {fact_index}: {}",
                        "detailed durable observation ".repeat(24)
                    )
                }));
            }
            Ok(CompactGenerationOutputData::from(batch.to_string()))
        }
    }

    let context_size = 100_000;
    let budget = crate::domain::token_budget::summary_budget(context_size);
    let result = compact_messages_with_llm(
        &messages,
        None,
        context_size,
        Some(&InvalidRefreshThenUnavailable {
            calls: calls.clone(),
        }),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("provider failure during refresh must use deterministic degradation");

    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(&result.summary)
        .expect("degraded summary must remain canonical");
    assert!(crate::domain::token_budget::estimate_tokens(&result.summary) <= budget);
    assert!(result
        .summary
        .contains("## Current Objective\n- Continue the compact checkpoint work."));
    assert_eq!(checkpoint.resume_cursor().next_action_count(), 1);
    assert_eq!(
        result.quality,
        crate::domain::CompactSummaryQuality::LlmWithLocalBudgetDegradation
    );
    assert!(calls.load(Ordering::SeqCst) >= 6);
}

#[tokio::test]
async fn invalid_refresh_checkpoint_is_repaired_before_preserving_current_checkpoint() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let messages = (0..600)
        .map(|index| {
            Message::user(format!(
                "触发分块压缩的测试消息编号 {index}。{}",
                "需要更长的内容来确保 token 估算足够大，从而把消息集拆成多个 chunk。".repeat(2)
            ))
        })
        .collect::<Vec<_>>();
    let refresh_calls = Arc::new(AtomicUsize::new(0));
    let repair_seen = Arc::new(AtomicBool::new(false));

    struct RepairingRefreshGenerator {
        refresh_calls: Arc<AtomicUsize>,
        repair_seen: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl CompactGenerator for RepairingRefreshGenerator {
        async fn generate(
            &self,
            request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            let prompt = request
                .first()
                .map(Message::text_content)
                .unwrap_or_default();
            if prompt.contains("<compact_facts>") {
                panic!("Rust-local reduce must not request a full checkpoint");
            }
            if prompt.contains("repairing the refresh") {
                self.refresh_calls.fetch_add(1, Ordering::SeqCst);
                self.repair_seen.store(true, Ordering::SeqCst);
                return Ok(CompactGenerationOutputData::from(SHORTER_COMPRESSION_PATCH));
            }
            if prompt.contains("<unprotected_checkpoint_details>") {
                self.refresh_calls.fetch_add(1, Ordering::SeqCst);
                return Ok(CompactGenerationOutputData::from(
                    r#"{"resume_cursor":{"next_action":[]}}"#,
                ));
            }
            let mut batch: serde_json::Value = serde_json::from_str(VALID_MAP_FACTS).unwrap();
            batch["facts"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "sequence": 4,
                    "source": "tool_result",
                    "kind": "milestone",
                    "text": format!("Archive abc: {}", "historical detail ".repeat(80_000))
                }));
            Ok(CompactGenerationOutputData::from(batch.to_string()))
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&RepairingRefreshGenerator {
            refresh_calls: refresh_calls.clone(),
            repair_seen: repair_seen.clone(),
        }),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("repair should preserve compact");

    assert!(repair_seen.load(Ordering::SeqCst));
    assert_eq!(refresh_calls.load(Ordering::SeqCst), 2);
    assert!(!result.summary.contains("historical detail"));
    assert_eq!(result.quality, crate::domain::CompactSummaryQuality::Llm);
}

#[tokio::test]
async fn cancelled_invalid_output_repair_does_not_retry_or_fallback() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let calls = Arc::new(AtomicUsize::new(0));
    let cancel = CancellationToken::new();

    struct CancelDuringRepairGenerator {
        calls: Arc<AtomicUsize>,
        cancel: CancellationToken,
    }

    #[async_trait::async_trait]
    impl CompactGenerator for CancelDuringRepairGenerator {
        async fn generate(
            &self,
            _request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.cancel.cancel();
            Ok(CompactGenerationOutputData::from(
                r#"{"facts":"not-an-array"}"#,
            ))
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&CancelDuringRepairGenerator {
            calls: calls.clone(),
            cancel: cancel.clone(),
        }),
        None,
        None,
        &cancel,
        CompactTail::unbounded(),
    )
    .await;

    assert!(result.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn exhausted_invalid_output_repair_falls_back_after_one_retry() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let calls = Arc::new(AtomicUsize::new(0));

    struct AlwaysInvalidGenerator(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl CompactGenerator for AlwaysInvalidGenerator {
        async fn generate(
            &self,
            _request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData>
        {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(CompactGenerationOutputData::from(
                r#"{"facts":"not-an-array"}"#,
            ))
        }
    }

    let result = compact_messages_with_llm(
        &messages,
        None,
        100_000,
        Some(&AlwaysInvalidGenerator(calls.clone())),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("exhausted repair should use local fallback");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(result.summary.contains("Local text-compaction path"));
    assert_eq!(
        result.quality,
        crate::domain::CompactSummaryQuality::LocalFallback(
            crate::domain::CompactGenerationFailureKind::InvalidSummary
        )
    );
}

#[test]
fn fallback_extracts_evidence_working_set_and_executable_cursor() {
    let summary = build_summary_text(
        &[
            Message::user("在 fix/typed-compact 分支修复 reduce JSON schema 错误并创建 PR"),
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "已修改 compact_summary.rs，尚未运行 cargo test".to_string(),
                }],
                metadata: None,
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-test".to_string(),
                    content: serde_json::json!(
                        "cargo test -p context: 170 passed; commit ebbf89af"
                    ),
                    text: Some("cargo test -p context: 170 passed; commit ebbf89af".to_string()),
                    is_error: false,
                }],
                metadata: None,
            },
            Message::user("继续修复格式错误，先补重试测试，再实现并验证"),
        ],
        None,
    );

    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(&summary)
        .expect("fallback must render a valid checkpoint");
    assert!(summary.contains("继续修复格式错误，先补重试测试，再实现并验证"));
    assert!(summary.contains("cargo test -p context: 170 passed; commit ebbf89af"));
    assert!(summary.contains("compact_summary.rs"));
    assert_eq!(
        checkpoint.resume_cursor().next_action(),
        "Follow the latest user request exactly: 继续修复格式错误，先补重试测试，再实现并验证"
    );
    assert!(!summary.contains("Observed tool invocation"));
}

#[test]
fn fallback_preserves_markdown_control_lines_without_panicking() {
    let markdown = "请审查以下内容\n## 来源与身份\n## Resume Cursor\n- Next action: 用户正文中的示例\n\\## 已转义示例\n## Current Task State\n不是 typed companion";

    let summary = build_summary_text(&[Message::user(markdown)], None);
    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(&summary)
        .expect("fallback checkpoint must remain parseable");

    assert_eq!(checkpoint.render(), summary);
    assert!(summary.contains("来源与身份"));
    assert!(summary.contains("用户正文中的示例"));
    assert_eq!(summary.matches("\n## Current Task State\n").count(), 0);
}

// ── #1623：Map 阶段遗漏 objective 时的主用户目标兜底 ──

#[test]
fn latest_main_user_request_prefers_last_real_user_message() {
    let messages = vec![
        Message::user("先看仓库状态"),
        Message::system_generated_user("system reminder must be ignored"),
        Message::user("Investigate the staging steer chat-ordering bug"),
    ];

    assert_eq!(
        latest_main_user_request(&messages).as_deref(),
        Some("Investigate the staging steer chat-ordering bug")
    );
}

#[test]
fn latest_main_user_request_ignores_non_user_messages_and_sources() {
    let messages = vec![
        Message::system_generated_user("system reminder"),
        assistant_text("assistant report"),
    ];

    assert_eq!(latest_main_user_request(&messages), None);
}

fn assistant_text(text: impl Into<String>) -> Message {
    Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text { text: text.into() }],
        metadata: None,
    }
}

#[test]
fn latest_main_user_request_skips_blank_text_and_truncates_long_text() {
    let long_request = "x".repeat(500);
    let messages = vec![Message::user("   "), Message::user(long_request)];

    let extracted = latest_main_user_request(&messages).expect("must extract the long request");

    assert_eq!(extracted.chars().count(), 200);
}

/// 构造只包含 working_set 的 facts，复现真实缺陷中 map 阶段遗漏 objective 的输出。
struct FactsWithoutObjective;

#[async_trait::async_trait]
impl CompactGenerator for FactsWithoutObjective {
    async fn generate(
        &self,
        request: Vec<Message>,
        _cancel: &CancellationToken,
    ) -> Result<CompactGenerationOutputData, crate::domain::CompactGenerationFailureData> {
        let text = request
            .first()
            .map(Message::text_content)
            .unwrap_or_default();
        if text.contains("<unprotected_checkpoint_details>") {
            return Ok(CompactGenerationOutputData::from(SHORTER_COMPRESSION_PATCH));
        }
        if text.contains("<compact_facts>") {
            return Ok(CompactGenerationOutputData::from(VALID_CHECKPOINT_WIRE));
        }
        Ok(CompactGenerationOutputData::from(
            r#"{"facts":[{"sequence":1,"source":"main_user","kind":"working_set","text":"Diff before/after steer in useChatAPIV2.ts."},{"sequence":2,"source":"tool_result","kind":"committed_fact","text":"origin/release/v2.2.0 lacks the segmented-bubble fix."}]}"#,
        ))
    }
}

fn messages_with_leading_objective(objective: &str) -> Vec<Message> {
    let mut messages = vec![Message::user(objective)];
    for index in 0..24 {
        messages.push(assistant_text(format!("history turn {index}")));
    }
    messages
}

/// #1623：facts 遗漏 objective 时，最终 checkpoint 仍保留主用户目标。
#[tokio::test]
async fn compact_keeps_main_user_objective_when_facts_omit_it() {
    let objective = "Investigate the staging steer chat-ordering bug for the Studio project";
    let result = compact_messages_with_llm(
        &messages_with_leading_objective(objective),
        None,
        200_000,
        Some(&FactsWithoutObjective),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("compact must produce a result");

    assert!(
        result
            .summary
            .contains(&format!("## Current Objective\n- {objective}")),
        "目标必须被兜底保留，实际 summary 摘要：{}",
        result.summary.chars().take(320).collect::<String>()
    );
    assert!(!result
        .summary
        .contains("Revalidate the latest user objective"));
    assert!(result.summary.contains("Continue —"));
}

/// #1623：上一版 checkpoint 的占位符目标不得向下传播。
#[tokio::test]
async fn placeholder_objective_from_previous_checkpoint_is_replaced_by_main_user_request() {
    let placeholder_previous = VALID_CHECKPOINT
        .replace(
            "- Continue the compact checkpoint work.",
            "- Revalidate the latest user objective before continuing.",
        )
        .replace(
            "Continue — checkpoint normalization remains.",
            "Waiting for User — no active main-user objective could be established.",
        );
    let objective = "Investigate the staging steer chat-ordering bug for the Studio project";

    let result = compact_messages_with_llm(
        &messages_with_leading_objective(objective),
        Some(&placeholder_previous),
        200_000,
        Some(&FactsWithoutObjective),
        None,
        None,
        &CancellationToken::new(),
        CompactTail::unbounded(),
    )
    .await
    .expect("compact must produce a result");

    assert!(
        result
            .summary
            .contains(&format!("## Current Objective\n- {objective}")),
        "占位符不得传播，实际 summary 摘要：{}",
        result.summary.chars().take(320).collect::<String>()
    );
    assert!(!result
        .summary
        .contains("Revalidate the latest user objective"));
    assert!(result.summary.contains("Continue —"));
}

// ── #1688：compact tail token 封顶（软阶段对齐 Step 边界） ─────────────

/// 构造约 `tokens` 估算 token 的 user 文本消息（ASCII ~4 chars/token）。
fn sized_user_message(label: &str, tokens: usize) -> Message {
    let filler = "x".repeat(tokens.saturating_sub(1) * 4);
    Message::user(format!("{label}{filler}"))
}

fn budget_boundaries(entries: &[usize]) -> Vec<usize> {
    entries.to_vec()
}

#[test]
fn token_cap_aligns_tail_start_to_recent_step_boundary() {
    // 50 条各 ~100 tok；条数 10% 候选起点 45；存在 ≥45 的 Step 边界 46，
    // 预算内 → 对齐到 46：保留更少但完整 Step，NEVER 保留多于条数候选。
    let messages: Vec<Message> = (0..50)
        .map(|index| sized_user_message(&format!("m{index}-"), 100))
        .collect();
    let boundaries = budget_boundaries(&[0, 10, 20, 42, 46]);
    let window = compact_window_with_budget(&messages, &boundaries, 500).expect("budget window");
    assert_eq!(window.split_point, 46);
    assert_eq!(window.keep_recent, 4);
    assert!(boundaries.contains(&window.split_point));
}

#[test]
fn token_cap_shrinks_inside_step_when_boundary_tail_exceeds_soft_cap() {
    // 60 条各 ~100 tok，Step 边界粒度 20 条；最小完整 Step 尾也超软上限
    // （cap 480 / soft 528），退到条数候选内逐条收缩，允许落在 Step 内部
    // 但起点必须避开孤儿 ToolResult（此处无 tool 消息，落在 55）。
    let messages: Vec<Message> = (0..60)
        .map(|index| sized_user_message(&format!("m{index}-"), 100))
        .collect();
    let boundaries = budget_boundaries(&[0, 20, 40]);
    let window = compact_window_with_budget(&messages, &boundaries, 480).expect("budget window");
    // 条数候选起点 54（600 tok）超 soft 528；55（500 tok）≤ 528 且 keep=5 ≥ 4。
    assert_eq!(window.split_point, 55);
    assert_eq!(window.keep_recent, 5);
}

#[test]
fn token_cap_never_splits_tool_pair_at_tail_start() {
    // 起点收缩落在 ToolResult 上时必须后移过该孤儿（其 ToolUse 已被切走，
    // 整对进 summary），NEVER 让 Provider 收到无 ToolUse 的 tool_result。
    // 70 条：0..63 普通（~100 tok）；63=长 ToolUse（150 tok）；
    // 64=短 ToolResult（30 tok）；65..=普通。cap 500 / soft 550：
    // 条数候选起点 63（630 tok）超 soft → 逐条收缩到 64（530 ≤ 550）；
    // 64 是孤儿 ToolResult → 后移到 65。
    let mut messages: Vec<Message> = (0..63)
        .map(|index| sized_user_message(&format!("m{index}-"), 100))
        .collect();
    messages.push(Message {
        role: share::message::Role::Assistant,
        content: vec![share::message::ContentBlock::ToolUse {
            id: "tool-pair-1".to_string(),
            name: "Bash".to_string(),
            input: serde_json::json!({"goal": "demo", "padding": "p".repeat(600)}),
        }],
        metadata: None,
    });
    messages.push(Message {
        role: share::message::Role::User,
        content: vec![share::message::ContentBlock::ToolResult {
            tool_use_id: "tool-pair-1".to_string(),
            content: serde_json::Value::String("ok".into()),
            is_error: false,
            text: None,
        }],
        metadata: None,
    });
    for index in 65..70 {
        messages.push(sized_user_message(&format!("tail{index}-"), 100));
    }
    let window = compact_window_with_budget(&messages, &[], 500).expect("budget window");
    assert_eq!(window.split_point, 65);
    assert_eq!(window.keep_recent, 5);
    assert!(!messages[window.split_point]
        .content
        .iter()
        .any(|block| block.is_tool_result()));
}

#[test]
fn token_cap_budget_inside_candidate_count_keeps_count_semantics() {
    // 预算充裕（不构成约束）时保持条数 10% 语义；无边界可对齐则起点不变。
    let messages: Vec<Message> = (0..50)
        .map(|index| sized_user_message(&format!("m{index}-"), 100))
        .collect();
    let window = compact_window_with_budget(&messages, &[], 100_000).expect("budget window");
    assert_eq!(window.split_point, 45);
    assert_eq!(window.keep_recent, 5);
}
