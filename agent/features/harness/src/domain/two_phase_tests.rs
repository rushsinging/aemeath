use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::errors::HarnessError;
use crate::domain::evidence::{EvidenceBundle, EvidenceSignal, EvidenceSourceRef, SignalKind};
use crate::domain::improvement::RsiLlmPort;
use crate::domain::lesson::LessonStore;
use crate::domain::test_support::FakeRegistry;
use async_trait::async_trait;
use share::config::RsiConfig;

fn tool_guidance() -> ComponentId {
    ComponentId {
        kind: ComponentKind::ToolGuidance,
        name: ComponentName::new("default"),
    }
}

fn body_content(body: &str) -> ComponentContent {
    ComponentContent {
        frontmatter: None,
        body: body.to_string(),
    }
}

fn evidence() -> EvidenceBundle {
    EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RepeatedFailure,
            signal_id: "repeated_failure:Bash:exit-##-code-1".to_string(),
            summary: "Bash 连续失败 ×2".to_string(),
            excerpt: Some("exit code 1".to_string()),
            occurrences: 3,
            sources: vec![EvidenceSourceRef {
                session_id: "s1.dataset".to_string(),
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
            }],
        }],
    }
}

struct TwoPhaseLlm {
    responses: Vec<String>,
    calls: std::sync::atomic::AtomicUsize,
    prompts: std::sync::Mutex<Vec<String>>,
}

impl TwoPhaseLlm {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses,
            calls: std::sync::atomic::AtomicUsize::new(0),
            prompts: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn prompt(&self, index: usize) -> String {
        self.prompts
            .lock()
            .expect("prompt lock")
            .get(index)
            .cloned()
            .unwrap_or_default()
    }
}

#[async_trait]
impl RsiLlmPort for TwoPhaseLlm {
    async fn complete(&self, prompt: &str) -> Result<String, HarnessError> {
        self.prompts
            .lock()
            .expect("prompt lock")
            .push(prompt.to_string());
        let index = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.responses
            .get(index)
            .cloned()
            .ok_or_else(|| HarnessError::unavailable(crate::constants::ERROR_DOMAIN, "桩耗尽"))
    }
}

#[tokio::test]
async fn two_phase_routes_candidate_then_executes_fixed_component() {
    let target = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![(target.clone(), body_content("old"))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Bash 连续失败需要工具纪律","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"Bash 失败后未诊断即重试","direction":"补充失败诊断与重试限制","route_reason":"通用 Bash 工具使用纪律"}]}"#.to_string(),
        r#"{"decision":"propose","reason":"补充失败诊断规则","evidence_refs":["E0"],"body":"Bash 失败后 MUST 先读取 stderr 并定位原因；未改变输入前 NEVER 原样重试。"}"#.to_string(),
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let outcome = crate::domain::improvement::run_two_phase_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        evidence(),
        &crate::domain::stream_port::FakeEvidenceStream::default(),
        false,
        &mut Vec::new(),
        None,
        None,
        &mut history,
        "cycle-test",
    )
    .await
    .expect("two-phase cycle");

    assert_eq!(outcome.outcome.summary, "采纳 1 / 无变更 0 / 作废 0");
    let diagnosis_prompt = llm.prompt(0);
    assert!(diagnosis_prompt.contains("\"scope\":\"global\""));
    assert!(diagnosis_prompt.contains("项目: unknown"));
    assert!(diagnosis_prompt.contains("scope=global 候选只承接跨项目通用规律"));
    assert!(diagnosis_prompt.contains("项目 A 证据 NEVER 选择项目 B 候选"));
    assert!(diagnosis_prompt.contains("工具参数类问题 NEVER 复述或改写工具 schema"));
    let execution_prompt = llm.prompt(1);
    assert!(execution_prompt.contains("目标 scope：global"));
    assert!(execution_prompt.contains("正文预算：3800 字符"));
    assert_eq!(outcome.outcome.adopted.len(), 1);
    assert_eq!(outcome.outcome.adopted[0].id, target);
    assert_eq!(
        outcome.outcome.adopted[0].evidence[0].signal_id,
        "repeated_failure:Bash:exit-##-code-1"
    );
    assert_eq!(
        registry
            .content_of(&target)
            .map(|content| content.body.clone()),
        Some(
            "Bash 失败后 MUST 先读取 stderr 并定位原因；未改变输入前 NEVER 原样重试。".to_string()
        )
    );
    let lessons = lesson_store.load_active().expect("lessons");
    assert_eq!(lessons.len(), 1);
    assert_eq!(
        lessons[0].verification.target_signal,
        "repeated_failure:Bash:exit-##-code-1"
    );
    assert!(lessons[0].evidence[0]
        .source
        .as_ref()
        .is_some_and(|source| source.step_id == "step-1"));
}

/// TwoPhase 执行超预算：MUST 追加一次 LLM 压缩，采用压缩稿；
/// NEVER 硬截断后落盘。
#[tokio::test]
async fn two_phase_oversized_body_gets_one_llm_compaction_pass() {
    let target = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![(target.clone(), body_content("old"))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let oversized = serde_json::json!({
        "decision": "propose",
        "reason": "初稿超预算",
        "evidence_refs": ["E0"],
        "body": "长".repeat(3801),
    })
    .to_string();
    let compact = serde_json::json!({
        "decision": "propose",
        "reason": "压缩后保留核心纪律",
        "evidence_refs": ["E0"],
        "body": "Bash 失败后 MUST 先诊断；无新信息 NEVER 原样重试。",
    })
    .to_string();
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Bash 连续失败","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"重复失败","direction":"收敛规则","route_reason":"工具纪律"}]}"#.to_string(),
        oversized,
        compact,
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let outcome = crate::domain::improvement::run_two_phase_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        evidence(),
        &crate::domain::stream_port::FakeEvidenceStream::default(),
        false,
        &mut Vec::new(),
        None,
        None,
        &mut history,
        "cycle-test",
    )
    .await
    .expect("two-phase cycle");

    assert_eq!(outcome.outcome.summary, "采纳 1 / 无变更 0 / 作废 0");
    assert_eq!(llm.call_count(), 3);
    assert_eq!(
        registry
            .content_of(&target)
            .map(|content| content.body.clone()),
        Some("Bash 失败后 MUST 先诊断；无新信息 NEVER 原样重试。".to_string())
    );
}

/// 压缩一轮仍超预算：整份提案作废，NEVER 硬截断落盘。
#[tokio::test]
async fn two_phase_rejects_body_still_oversized_after_compaction() {
    let target = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![(target.clone(), body_content("old"))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let oversized = serde_json::json!({
        "decision": "propose",
        "reason": "仍超预算",
        "evidence_refs": ["E0"],
        "body": "长".repeat(3801),
    })
    .to_string();
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Bash 连续失败","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"重复失败","direction":"收敛规则","route_reason":"工具纪律"}]}"#.to_string(),
        oversized.clone(),
        oversized,
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let outcome = crate::domain::improvement::run_two_phase_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        evidence(),
        &crate::domain::stream_port::FakeEvidenceStream::default(),
        false,
        &mut Vec::new(),
        None,
        None,
        &mut history,
        "cycle-test",
    )
    .await
    .expect("two-phase cycle");

    assert_eq!(outcome.outcome.summary, "采纳 0 / 无变更 0 / 作废 1");
    assert_eq!(llm.call_count(), 3);
    assert_eq!(
        registry
            .content_of(&target)
            .map(|content| content.body.clone()),
        Some("old".to_string())
    );
    assert!(lesson_store.load_active().expect("lessons").is_empty());
}

#[tokio::test]
async fn two_phase_dry_run_previews_without_persisting() {
    let target = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![(target.clone(), body_content("old"))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Bash 连续失败需要工具纪律","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"Bash 失败后未诊断即重试","direction":"补充规则","route_reason":"工具纪律"}]}"#.to_string(),
        r#"{"decision":"propose","reason":"补充规则","evidence_refs":["E0"],"body":"new body"}"#.to_string(),
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let mut preview = Vec::new();
    let mut virtual_registry =
        crate::domain::virtual_registry::VirtualComponentRegistry::new(&mut registry);
    let result = crate::domain::improvement::run_two_phase_cycle(
        &mut virtual_registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        evidence(),
        &crate::domain::stream_port::FakeEvidenceStream::default(),
        true,
        &mut preview,
        None,
        None,
        &mut history,
        "cycle-dry",
    )
    .await
    .expect("two-phase dry-run");
    assert_eq!(
        result.outcome.summary,
        "（dry-run）采纳 1 / 无变更 0 / 作废 0"
    );
    assert_eq!(
        registry
            .content_of(&target)
            .map(|content| content.body.clone()),
        Some("old".to_string())
    );
    assert!(lesson_store.load_active().expect("lessons").is_empty());
    assert!(preview
        .iter()
        .any(|line| line.contains("[dry-run][two-phase] upsert tool-guidance/default")));
}
