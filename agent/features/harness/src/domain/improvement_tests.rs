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
    let prompt = build_block_prompt(&chunk, &[]);
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
    let outcome = run_improvement_cycle(&mut registry, &llm, &lesson_store, &config(1))
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
    let outcome = run_improvement_cycle(&mut registry, &llm, &lesson_store, &config(1))
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
    let outcome = run_improvement_cycle(&mut registry, &llm, &lesson_store, &config(4))
        .await
        .expect("空注册表 NEVER 报错");
    assert!(outcome.summary.contains("空周期"));
}
