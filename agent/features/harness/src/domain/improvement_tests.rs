//! 改进循环契约测试：分块、提案解析、块级幂等与汇总。

use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::errors::HarnessError;
use crate::domain::improvement::{
    build_block_prompt, chunk_materials, parse_block_proposal, run_improvement_cycle,
    BlockProposal, ImprovementMaterials, RsiLlmPort,
};
use crate::domain::test_support::FakeRegistry;
use async_trait::async_trait;
use share::config::RsiConfig;

fn id(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::Skill,
        name: ComponentName::new(name),
    }
}

fn body_content(body: &str) -> ComponentContent {
    ComponentContent {
        frontmatter: None,
        body: body.to_string(),
    }
}

fn config(chunk: usize) -> RsiConfig {
    RsiConfig {
        chunk_max_components: chunk,
        ..RsiConfig::default()
    }
}

fn evidence_with_signal() -> crate::domain::evidence::EvidenceBundle {
    crate::domain::evidence::EvidenceBundle {
        sessions_covered: 1,
        signals: vec![crate::domain::evidence::EvidenceSignal {
            kind: crate::domain::evidence::SignalKind::RepeatedFailure,
            signal_id: "repeated_failure:Bash:cargo build failed".into(),
            summary: "工具 Bash 重复失败 ×2".into(),
            excerpt: Some("cargo build failed".into()),
            occurrences: 2,
            sources: vec![crate::domain::evidence::EvidenceSourceRef {
                session_id: "s1.dataset".into(),
                run_id: "run-1".into(),
                step_id: "step-1".into(),
            }],
        }],
    }
}

/// 应答可编程的 LLM 桩。
struct ScriptedLlm {
    responses: Vec<String>,
    calls: std::sync::atomic::AtomicUsize,
}

impl ScriptedLlm {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses,
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl RsiLlmPort for ScriptedLlm {
    async fn complete(&self, _prompt: &str) -> Result<String, HarnessError> {
        let index = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.responses
            .get(index)
            .cloned()
            .ok_or_else(|| HarnessError::unavailable(crate::constants::ERROR_DOMAIN, "桩耗尽"))
    }
}

/// 分块：按 chunk_max_components 分组、上限至少 1。
#[test]
fn materials_chunked_by_configured_size() {
    let materials = ImprovementMaterials {
        lessons: vec![],
        evidence: crate::domain::evidence::EvidenceBundle::default(),
        component_contents: vec![
            (id("a"), body_content("1")),
            (id("b"), body_content("2")),
            (id("c"), body_content("3")),
            (id("d"), body_content("4")),
            (id("e"), body_content("5")),
        ],
    };
    let chunks = chunk_materials(materials, 2);
    assert_eq!(
        chunks.iter().map(|chunk| chunk.len()).collect::<Vec<_>>(),
        vec![2, 2, 1],
    );
}

/// 提案信封解析：propose / no_change 两态。
#[test]
fn block_proposal_envelope_parses() {
    let propose = parse_block_proposal(
        r#"{"decision":"propose","reason":"依据","signal_ids":["repeated_failure:Bash:cargo build failed"],"change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"x"},"content":{"frontmatter":null,"body":"new"}}]}}"#,
    )
    .expect("propose 必须可解析");
    match propose {
        BlockProposal::Propose { signal_ids, .. } => assert_eq!(
            signal_ids,
            vec!["repeated_failure:Bash:cargo build failed".to_string()]
        ),
        other => panic!("预期 propose：{other:?}"),
    }

    let no_change = parse_block_proposal(r#"{"decision":"no_change","reason":"证据不足"}"#)
        .expect("no_change 必须可解析");
    assert!(matches!(no_change, BlockProposal::NoChange { .. }));

    assert!(parse_block_proposal("我觉得不需要改").is_err());
}

/// prompt 含组件材料与编辑契约要点。
#[test]
fn block_prompt_carries_materials_and_contract() {
    let chunk = vec![(id("alpha"), body_content("正文内容"))];
    let prompt = build_block_prompt(
        &chunk,
        &[],
        &crate::domain::evidence::EvidenceBundle::default(),
    );
    assert!(prompt.contains("alpha"));
    assert!(prompt.contains("正文内容"));
    assert!(prompt.contains("no_change"));
    assert!(prompt.contains("signal_ids"));
    assert!(prompt.contains("未提及的组件自动保持原样"));
}

/// 完整周期：两块（采纳 + 无变更）→ 汇总正确、未提及组件继承。
#[tokio::test]
async fn cycle_adopts_and_summarizes_per_block() {
    let mentioned = id("mentioned");
    let untouched = id("untouched");
    let mut registry = FakeRegistry::with_entries(vec![
        (mentioned.clone(), body_content("old")),
        (untouched.clone(), body_content("keep")),
    ]);
    // 单阶段（#2078 修订）：每块一次提案调用
    let llm = ScriptedLlm::new(vec![
        r#"{"decision":"propose","reason":"正文过时","signal_ids":["repeated_failure:Bash:cargo build failed"],"change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"mentioned"},"content":{"frontmatter":null,"body":"new"}}]}}"#.to_string(),
        r#"{"decision":"no_change","reason":"证据不足"}"#.to_string(),
    ]);

    let lesson_store = crate::domain::lesson::LessonStore::new(
        tempfile::tempdir()
            .expect("tempdir")
            .path()
            .join("library.jsonl"),
    );
    let outcome = run_improvement_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &config(1),
        evidence_with_signal(),
        false,
        &mut Vec::new(),
        None,
        None,
    )
    .await
    .expect("周期必须成功");
    assert!(outcome.summary.contains("采纳 1"));
    assert!(outcome.summary.contains("无变更 1"));
    // 采纳块 MUST 回写带证据的 Active lesson（复利输入）
    let lessons = lesson_store.load_active().expect("lessons");
    assert_eq!(lessons.len(), 1);
    assert!(!lessons[0].evidence.is_empty());
    assert_eq!(
        lessons[0].verification.target_signal,
        "repeated_failure:Bash:cargo build failed"
    );
    assert_eq!(
        lessons[0].evidence[0].signal_id,
        "repeated_failure:Bash:cargo build failed"
    );
    assert!(lessons[0].evidence[0]
        .source
        .as_ref()
        .is_some_and(|source| source.step_id == "step-1"));
    assert_eq!(
        outcome.adopted[0].evidence[0].signal_id,
        "repeated_failure:Bash:cargo build failed"
    );
    assert!(outcome.adopted[0].evidence[0]
        .source
        .as_ref()
        .is_some_and(|source| source.run_id == "run-1"));
    assert_eq!(
        registry
            .content_of(&mentioned)
            .map(|content| content.body.clone()),
        Some("new".to_string()),
    );
    assert_eq!(
        registry
            .content_of(&untouched)
            .map(|content| content.body.clone()),
        Some("keep".to_string()),
    );
}

/// 块级幂等：一块解析失败（坏 JSON）只作废该块，其余块照常采纳。
#[tokio::test]
async fn malformed_block_discarded_without_blocking_others() {
    let good = id("good");
    let mut registry = FakeRegistry::with_entries(vec![
        (id("bad"), body_content("x")),
        (good.clone(), body_content("old")),
    ]);
    let llm = ScriptedLlm::new(vec![
        "这不是 JSON".to_string(),
        r#"{"decision":"propose","reason":"正文过时","signal_ids":["repeated_failure:Bash:cargo build failed"],"change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"good"},"content":{"frontmatter":null,"body":"new"}}]}}"#.to_string(),
    ]);

    let lesson_store = crate::domain::lesson::LessonStore::new(
        tempfile::tempdir()
            .expect("tempdir")
            .path()
            .join("library.jsonl"),
    );
    let outcome = run_improvement_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &config(1),
        evidence_with_signal(),
        false,
        &mut Vec::new(),
        None,
        None,
    )
    .await
    .expect("周期必须成功");
    assert!(outcome.summary.contains("作废 1"));
    assert!(outcome.summary.contains("采纳 1"));
    assert_eq!(
        registry
            .content_of(&good)
            .map(|content| content.body.clone()),
        Some("new".to_string()),
    );
}

/// 空注册表：空周期（合法结果，NEVER 报错）。
#[tokio::test]
async fn empty_registry_yields_empty_cycle() {
    let mut registry = FakeRegistry::with_entries(vec![]);
    let llm = ScriptedLlm::new(vec![]);
    let lesson_store = crate::domain::lesson::LessonStore::new(
        tempfile::tempdir()
            .expect("tempdir")
            .path()
            .join("library.jsonl"),
    );
    let outcome = run_improvement_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &config(4),
        Default::default(),
        false,
        &mut Vec::new(),
        None,
        None,
    )
    .await
    .expect("空注册表 NEVER 报错");
    assert!(outcome.summary.contains("空周期"));
}

// ---- #2048 evidence 材料注入 ----

/// prompt 含 evidence 块（signal id 可引用）与引用约束；空证据兜底。
#[test]
fn block_prompt_carries_evidence_block_and_reference_rule() {
    use crate::domain::evidence::{
        aggregate_evidence, EvidenceSignal, SessionEventRecord, SignalKind,
    };
    let chunk = vec![(id("alpha"), body_content("正文"))];
    let events = vec![
        SessionEventRecord {
            session_id: "s".into(),
            run_id: "r".into(),
            step_id: "step-1".into(),
            tool_name: Some("Bash".into()),
            is_error: true,
            error_text: Some("exit code 1: boom".into()),
            result_chars: 10,
            call_fingerprint: None,
        };
        2
    ];
    let evidence = aggregate_evidence(&events);
    let prompt = build_block_prompt(&chunk, &[], &evidence);
    assert!(prompt.contains("repeated_failure:Bash:"));
    assert!(
        prompt.contains("MUST 在 signal_ids"),
        "引用约束必须在 prompt"
    );

    let empty_prompt = build_block_prompt(&chunk, &[], &Default::default());
    assert!(empty_prompt.contains("（近期无执行证据）"), "空证据兜底");

    // 优先级渲染顺序
    let bundle_with_both = crate::domain::evidence::EvidenceBundle {
        signals: vec![
            EvidenceSignal {
                kind: SignalKind::RunOutcome,
                signal_id: "run_outcome:aggregate".into(),
                summary: "终态".into(),
                excerpt: None,
                occurrences: 1,
                sources: Vec::new(),
            },
            EvidenceSignal {
                kind: SignalKind::RepeatedFailure,
                signal_id: "repeated_failure:Bash:x".into(),
                summary: "失败".into(),
                excerpt: None,
                occurrences: 1,
                sources: Vec::new(),
            },
        ],
        sessions_covered: 1,
    };
    let both_prompt = build_block_prompt(&chunk, &[], &bundle_with_both);
    let failure_pos = both_prompt.find("repeated_failure:Bash:x").unwrap();
    let outcome_pos = both_prompt.find("run_outcome:aggregate").unwrap();
    assert!(failure_pos < outcome_pos, "repeated_failure 渲染在前");
}

/// 提案 signal 引用门禁：未知引用整份作废，NEVER 落盘或回写 lesson。
#[tokio::test]
async fn unknown_signal_reference_discards_whole_proposal() {
    let mut registry = FakeRegistry::with_entries(vec![(id("alpha"), body_content("old"))]);
    let llm = ScriptedLlm::new(vec![
        r#"{"decision":"propose","reason":"引用不存在信号","signal_ids":["repeated_failure:Grep:missing"],"change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"alpha"},"content":{"frontmatter":null,"body":"new"}}]}}"#.to_string(),
    ]);
    let lesson_store = crate::domain::lesson::LessonStore::new(
        tempfile::tempdir()
            .expect("tempdir")
            .path()
            .join("library.jsonl"),
    );
    let outcome = run_improvement_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &config(4),
        evidence_with_signal(),
        false,
        &mut Vec::new(),
        None,
        None,
    )
    .await
    .expect("周期必须成功");
    assert!(
        outcome.summary.contains("作废 1"),
        "未知 signal 引用必须作废：{}",
        outcome.summary
    );
    assert!(lesson_store.load_active().expect("lessons").is_empty());
    assert_eq!(
        registry
            .content_of(&id("alpha"))
            .map(|content| content.body.clone()),
        Some("old".to_string())
    );
}

// ---- Compose 相似候选与 session 上下文 ----

fn adoption_record(
    name: &str,
    reason: &str,
    signal_id: &str,
    source: Option<crate::domain::evidence::EvidenceSourceRef>,
) -> crate::domain::task::AdoptionRecord {
    crate::domain::task::AdoptionRecord {
        id: id(name),
        reason: reason.to_string(),
        evidence: vec![crate::domain::lesson::EvidenceBinding {
            run_ref: "cycle-1".to_string(),
            signal: crate::domain::evidence::SignalKind::RepeatedFailure,
            excerpt: reason.chars().take(200).collect(),
            signal_id: signal_id.to_string(),
            source,
        }],
    }
}

fn active_lesson(
    name: &str,
    lesson: &str,
    signal_id: &str,
    source: Option<crate::domain::evidence::EvidenceSourceRef>,
) -> crate::domain::lesson::LessonEntry {
    crate::domain::lesson::LessonEntry::new(
        format!("lesson-{name}"),
        "guardrails",
        lesson.to_string(),
        vec![crate::domain::lesson::EvidenceBinding {
            run_ref: "cycle-old".to_string(),
            signal: crate::domain::evidence::SignalKind::RepeatedFailure,
            excerpt: lesson.chars().take(200).collect(),
            signal_id: signal_id.to_string(),
            source,
        }],
        1,
    )
    .expect("lesson")
}

fn source_ref(step_id: &str) -> crate::domain::evidence::EvidenceSourceRef {
    crate::domain::evidence::EvidenceSourceRef {
        session_id: "s1.dataset".to_string(),
        run_id: "run-1".to_string(),
        step_id: step_id.to_string(),
    }
}

fn fake_stream_with_context(
    step_id: &str,
    text: &str,
) -> crate::domain::stream_port::FakeEvidenceStream {
    let stream = crate::domain::stream_port::FakeEvidenceStream::default();
    stream.contexts.lock().expect("contexts").push(
        crate::domain::evidence::SessionContextExcerpt {
            source: source_ref(step_id),
            text: text.to_string(),
        },
    );
    stream
}

/// 中文提案/lesson 必须按 CJK bigram 召回，不能因整句单 token 漏检。
#[test]
fn similar_candidates_recall_chinese_text_with_context() {
    let proposal = adoption_record(
        "alpha",
        "编辑前必须先读取文件内容并确认上下文",
        "repeated_failure:Edit:old_string",
        Some(source_ref("step-1")),
    );
    let lesson = active_lesson(
        "old",
        "编辑文件前应读取目标内容，确认上下文后再落笔",
        "repeated_failure:Edit:old_string",
        Some(source_ref("step-2")),
    );
    let stream = fake_stream_with_context("step-1", "提案来源上下文：修改 x.rs");
    let pairs = crate::domain::improvement::select_similar_candidate_pairs(
        std::slice::from_ref(&proposal),
        std::slice::from_ref(&lesson),
        crate::constants::COMPOSE_SIMILAR_PAIR_LIMIT,
        &stream,
    );
    assert_eq!(pairs.len(), 1, "同 signal 的中文候选必须召回");
    assert!(pairs[0].score >= crate::constants::COMPOSE_SIMILARITY_CANDIDATE_THRESHOLD);
    assert!(
        pairs[0].left_context.is_some(),
        "候选必须回读来源 session 上下文"
    );
}

/// 语义无关的提案不应进入候选，避免无意义上下文注入。
#[test]
fn unrelated_candidates_are_not_recalled() {
    let proposal = adoption_record(
        "alpha",
        "编辑前必须先读取文件内容并确认上下文",
        "repeated_failure:Edit:old_string",
        Some(source_ref("step-1")),
    );
    let lesson = active_lesson(
        "old",
        "网络请求超时后应降低并发并检查凭据",
        "run_outcome:aggregate",
        None,
    );
    let stream = crate::domain::stream_port::FakeEvidenceStream::default();
    let pairs = crate::domain::improvement::select_similar_candidate_pairs(
        std::slice::from_ref(&proposal),
        std::slice::from_ref(&lesson),
        crate::constants::COMPOSE_SIMILAR_PAIR_LIMIT,
        &stream,
    );
    assert!(pairs.is_empty(), "无关候选不得注入：{pairs:?}");
}

/// Compose prompt 必须携带候选对、双方上下文与缺失/不可合并规则。
#[test]
fn compose_similarity_block_carries_context_and_guardrails() {
    let mut pair = crate::domain::improvement::SimilarCandidatePair {
        left_label: "本周期提案：alpha".to_string(),
        left_text: "编辑前先读取".to_string(),
        right_label: "Active lesson：lesson-old".to_string(),
        right_text: "落笔前确认上下文".to_string(),
        left_context: Some("来源上下文 A：Bash 失败".to_string()),
        right_context: None,
        score: 0.62,
    };
    let block = crate::domain::improvement::render_compose_similarity_block(std::slice::from_mut(
        &mut pair,
    ));
    assert!(block.contains("本周期提案：alpha"));
    assert!(block.contains("Active lesson：lesson-old"));
    assert!(block.contains("来源上下文 A：Bash 失败"));
    assert!(block.contains("上下文不可用"), "缺失上下文必须显式标注");
    assert!(block.contains("相似度只是召回线索"));
    assert!(block.contains("上下文不同或缺失"), "必须禁止据此合并");
}

/// Compose 相似上下文总预算有界：超长候选上下文被截断。
#[test]
fn compose_similarity_context_total_budget_is_bounded() {
    let stream = crate::domain::stream_port::FakeEvidenceStream::default();
    {
        let mut contexts = stream.contexts.lock().expect("contexts");
        for index in 0..6 {
            contexts.push(crate::domain::evidence::SessionContextExcerpt {
                source: source_ref(&format!("step-{index}")),
                text: "长".repeat(3000),
            });
        }
    }
    let adoptions: Vec<_> = (0..6)
        .map(|index| {
            adoption_record(
                &format!("alpha{index}"),
                "编辑前必须先读取文件内容并确认上下文",
                "repeated_failure:Edit:old_string",
                Some(source_ref(&format!("step-{index}"))),
            )
        })
        .collect();
    let pairs = crate::domain::improvement::select_similar_candidate_pairs(
        &adoptions,
        &[],
        crate::constants::COMPOSE_SIMILAR_PAIR_LIMIT,
        &stream,
    );
    let total: usize = pairs
        .iter()
        .flat_map(|pair| [pair.left_context.as_deref(), pair.right_context.as_deref()])
        .flatten()
        .map(|text| text.chars().count())
        .sum();
    assert!(
        total <= crate::constants::COMPOSE_SIMILAR_CONTEXT_TOTAL_CHARS,
        "相似上下文总预算超限：{total}"
    );
}

/// Compose 端到端装配：Active lesson 与 session 上下文必须进入真实 prompt。
#[tokio::test]
async fn compose_phase_passes_active_lessons_and_context_to_prompt() {
    struct RecordingLlm {
        prompt: std::sync::Mutex<String>,
    }
    #[async_trait]
    impl RsiLlmPort for RecordingLlm {
        async fn complete(&self, prompt: &str) -> Result<String, HarnessError> {
            *self.prompt.lock().expect("prompt lock") = prompt.to_string();
            Ok(r#"{"decision":"no_change","reason":"无矛盾"}"#.to_string())
        }
    }

    let mut registry = FakeRegistry::with_entries(vec![(id("alpha"), body_content("规则"))]);
    let proposal = adoption_record(
        "alpha",
        "编辑前先读取文件内容",
        "repeated_failure:Edit:old_string",
        Some(source_ref("step-1")),
    );
    let lesson = active_lesson(
        "old",
        "落笔前先读取文件并确认上下文",
        "repeated_failure:Edit:old_string",
        Some(source_ref("step-2")),
    );
    let stream = fake_stream_with_context("step-1", "来源上下文：Edit old_string 不匹配");
    stream.contexts.lock().expect("contexts").push(
        crate::domain::evidence::SessionContextExcerpt {
            source: source_ref("step-2"),
            text: "历史上下文：同场景反复失败".to_string(),
        },
    );
    let llm = RecordingLlm {
        prompt: std::sync::Mutex::new(String::new()),
    };
    let lesson_store = crate::domain::lesson::LessonStore::new(
        tempfile::tempdir()
            .expect("tempdir")
            .path()
            .join("library.jsonl"),
    );
    let outcome = crate::domain::improvement::run_compose_phase(
        &mut registry,
        &llm,
        &lesson_store,
        &[proposal],
        std::slice::from_ref(&lesson),
        &stream,
        false,
        &mut Vec::new(),
        None,
        None,
    )
    .await
    .expect("Compose 必须成功")
    .expect("有采纳时 Compose 必须产出结果");
    assert!(outcome.summary.contains("无变更"), "{}", outcome.summary);
    let prompt = llm.prompt.lock().expect("prompt lock").clone();
    assert!(
        prompt.contains("Active lesson"),
        "prompt 缺 lesson：{prompt}"
    );
    assert!(prompt.contains("候选 1"), "prompt 缺候选对：{prompt}");
    assert!(
        prompt.contains("来源上下文：Edit old_string 不匹配"),
        "prompt 缺提案来源上下文：{prompt}"
    );
    assert!(
        prompt.contains("历史上下文：同场景反复失败"),
        "prompt 缺 lesson 来源上下文：{prompt}"
    );
}

// ---- dry-run 富预览（实在总结：前后规模 + 要点 + 依据）----

#[test]
fn dry_run_edit_summary_carries_scale_and_highlights() {
    use crate::adapters::filesystem::FilesystemComponentRegistry;
    use std::collections::HashMap;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("g.md");
    std::fs::write(&path, "TODO: 待补充。\n").expect("write");
    let mut registry_map: HashMap<ComponentId, std::path::PathBuf> = HashMap::new();
    registry_map.insert(id("alpha"), path.clone());
    let mut registry = FilesystemComponentRegistry::new(registry_map);

    let content = ComponentContent {
        frontmatter: None,
        body: "# 执行规则\n## 编辑防护\n正文行。\n".to_string(),
    };
    let summary = crate::domain::improvement::render_dry_run_edit_summary(
        &mut registry,
        &id("alpha"),
        Some(&content),
        "回应 repeated_failure:Edit:x",
    );
    assert!(summary.contains("upsert"));
    assert!(summary.contains("1 行 → 3 行"), "前后规模：{summary}");
    assert!(
        summary.contains("# 执行规则 / ## 编辑防护"),
        "标题要点：{summary}"
    );
    assert!(
        summary.contains("依据：回应 repeated_failure"),
        "依据：{summary}"
    );
}
