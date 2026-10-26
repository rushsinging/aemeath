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
        r#"{"decision":"propose","reason":"依据","change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"x"},"content":{"frontmatter":null,"body":"new"}}]}}"#,
    )
    .expect("propose 必须可解析");
    assert!(matches!(propose, BlockProposal::Propose { .. }));

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
        r#"{"decision":"propose","reason":"正文过时","change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"mentioned"},"content":{"frontmatter":null,"body":"new"}}]}}"#.to_string(),
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
        Default::default(),
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
        r#"{"decision":"propose","reason":"正文过时","change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"good"},"content":{"frontmatter":null,"body":"new"}}]}}"#.to_string(),
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
        Default::default(),
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
    assert!(prompt.contains("MUST 指出它回应"), "引用约束必须在 prompt");

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
            },
            EvidenceSignal {
                kind: SignalKind::RepeatedFailure,
                signal_id: "repeated_failure:Bash:x".into(),
                summary: "失败".into(),
                excerpt: None,
            },
        ],
        sessions_covered: 1,
    };
    let both_prompt = build_block_prompt(&chunk, &[], &bundle_with_both);
    let failure_pos = both_prompt.find("repeated_failure:Bash:x").unwrap();
    let outcome_pos = both_prompt.find("run_outcome:aggregate").unwrap();
    assert!(failure_pos < outcome_pos, "repeated_failure 渲染在前");
}

/// lesson 绑定：reason 引用 signal id 时绑定真实 kind。
#[test]
fn lesson_signal_kind_bound_from_reason_reference() {
    assert_eq!(
        crate::domain::improvement::signal_kind_from_reason(
            "回应 repeated_failure:Bash:exit-##-code 的证据"
        ),
        crate::domain::evidence::SignalKind::RepeatedFailure
    );
    assert_eq!(
        crate::domain::improvement::signal_kind_from_reason("回应 retry_pattern:Edit:f 的证据"),
        crate::domain::evidence::SignalKind::RetryPattern
    );
    assert_eq!(
        crate::domain::improvement::signal_kind_from_reason("组件是空壳占位"),
        crate::domain::evidence::SignalKind::CycleObservation,
        "无引用兜底模型自述"
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
