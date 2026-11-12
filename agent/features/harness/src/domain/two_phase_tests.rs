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
            sources: vec![
                EvidenceSourceRef {
                    session_id: "0123456789abcdef/s1.dataset".to_string(),
                    run_id: "run-1".to_string(),
                    step_id: "step-1".to_string(),
                },
                EvidenceSourceRef {
                    session_id: "fedcba9876543210/s2.dataset".to_string(),
                    run_id: "run-1".to_string(),
                    step_id: "step-1".to_string(),
                },
            ],
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

fn single_project_evidence() -> EvidenceBundle {
    EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RepeatedFailure,
            signal_id: "repeated_failure:Bash:project-only".to_string(),
            summary: "Bash 连续失败 ×2".to_string(),
            excerpt: Some("exit code 1".to_string()),
            occurrences: 3,
            sources: vec![EvidenceSourceRef {
                session_id: "0123456789abcdef/s1.dataset".to_string(),
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
            }],
        }],
    }
}

/// 跨 scope 硬门禁：project overlay 新提案复述 global tool guidance 时，
/// 宿主 MUST 拒绝整份提案且零落盘。
#[tokio::test]
async fn two_phase_patch_updates_only_anchored_fragment() {
    let target = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![(
        target.clone(),
        body_content("# Tool Guidance\nold rule\nkeep tail\n"),
    )]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Bash 失败","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"重试纪律","direction":"补 patch","route_reason":"工具纪律"}]}"#.to_string(),
        serde_json::json!({
            "decision": "propose",
            "reason": "只替换目标行",
            "evidence_refs": ["E0"],
            "operations": [{"op":"replace_exact","old_string":"old rule","new_string":"new rule"}]
        })
        .to_string(),
        r#"{"verdicts":[]}"#.to_string(),
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let outcome = crate::domain::improvement::run_two_phase_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        single_project_evidence(),
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
    assert_eq!(
        registry
            .content_of(&target)
            .map(|content| content.body.clone()),
        Some("# Tool Guidance\nnew rule\nkeep tail\n".to_string()),
        "patch 只替换锚点，其余正文必须保留"
    );
    assert_eq!(outcome.outcome.adopted[0].id, target, "patch 仍绑定原组件");
}

#[tokio::test]
async fn two_phase_rejects_same_component_semantic_duplicate() {
    let target = tool_guidance();
    let original =
        "# Tool Guidance\n- 同一调用签名不得原样重复；重试前必须核对错误、参数与状态。\n";
    let mut registry = FakeRegistry::with_entries(vec![(target.clone(), body_content(original))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Bash 失败","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"重试纪律","direction":"补 patch","route_reason":"工具纪律"}]}"#.to_string(),
        serde_json::json!({
            "decision": "propose",
            "reason": "补重试纪律",
            "evidence_refs": ["E0"],
            "operations": [{"op":"replace_exact","old_string":"- 同一调用签名不得原样重复；重试前必须核对错误、参数与状态。","new_string":"- 同一调用签名不得原样重复；重试前必须核对错误、参数与状态。\n- Bash 同签名重复调用不得再次发起。"}]
        })
        .to_string(),
        r#"{"verdicts":[{"index":0,"verdict":"duplicate","reason":"新增规则是已有重试纪律的复述"}]}"#.to_string(),
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let outcome = crate::domain::improvement::run_two_phase_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        single_project_evidence(),
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
    assert_eq!(
        registry
            .content_of(&target)
            .map(|content| content.body.clone()),
        Some(original.to_string()),
        "同组件语义重复 MUST 整份拒绝，且不写入"
    );
}

#[tokio::test]
async fn two_phase_tool_signal_never_writes_project_overlay() {
    let project = ComponentId {
        kind: ComponentKind::SystemPromptOverlay,
        name: ComponentName::new("0123456789abcdef"),
    };
    let global = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![
        (global.clone(), body_content("# Tool Guidance\nold\n")),
        (project.clone(), body_content("# Project\n")),
    ]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Read 超长","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"Read 超长","direction":"分段读取","route_reason":"工具纪律"}]}"#.to_string(),
        serde_json::json!({
            "decision": "propose",
            "reason": "补充读取规则",
            "evidence_refs": ["E0"],
            "body": "# Tool Guidance\n- Read 超长时先分段、过滤或摘要。\n"
        })
        .to_string(),
        r#"{"verdicts":[]}"#.to_string(),
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let outcome = crate::domain::improvement::run_two_phase_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        single_project_evidence(),
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
    assert!(llm.prompt(1).contains("目标组件：tool-guidance/default"));
    assert_eq!(
        registry
            .content_of(&project)
            .map(|content| content.body.clone()),
        Some("# Project\n".to_string()),
        "工具信号 NEVER 写项目 overlay"
    );
    assert_eq!(
        registry
            .content_of(&global)
            .map(|content| content.body.clone()),
        Some("# Tool Guidance\n- Read 超长时先分段、过滤或摘要。\n".to_string())
    );
}

/// 存量收敛：结构化判定为 duplicate 且权威在 global 时，宿主只删除项目侧
/// 重复规则行；global 语义保留，且不生成半截正文。
#[tokio::test]
async fn two_phase_reconciles_existing_cross_scope_duplicate() {
    let project = ComponentId {
        kind: ComponentKind::SystemPromptOverlay,
        name: ComponentName::new("0123456789abcdef"),
    };
    let global = tool_guidance();
    let global_body = "# Tool Guidance\n- Read 超长时先分段、过滤或摘要，只保留诊断片段。\n";
    let mut registry = FakeRegistry::with_entries(vec![
        (global.clone(), body_content(global_body)),
        (
            project.clone(),
            body_content(
                "# Project\n- Read 超长时先分段、过滤或摘要，只保留诊断片段。\n- 保留项目差异。\n",
            ),
        ),
    ]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let llm = TwoPhaseLlm::new(vec![
        r#"{"verdicts":[{"index":0,"verdict":"duplicate","authority":"global","reason":"项目复述全局"}]}"#.to_string(),
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let outcome = crate::domain::improvement::run_two_phase_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        EvidenceBundle {
            sessions_covered: 1,
            signals: vec![EvidenceSignal {
                kind: SignalKind::RunOutcome,
                signal_id: "run_outcome:aggregate".to_string(),
                summary: "观察".to_string(),
                excerpt: None,
                occurrences: 1,
                sources: Vec::new(),
            }],
        },
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

    assert_eq!(outcome.outcome.summary, "无候选组件：仅观察");
    assert_eq!(
        registry
            .content_of(&project)
            .map(|content| content.body.clone()),
        Some("# Project\n- 保留项目差异。\n".to_string())
    );
    assert_eq!(
        registry
            .content_of(&global)
            .map(|content| content.body.clone()),
        Some(global_body.to_string())
    );
}

/// 权威在项目时，存量收敛 MUST 下沉规则：删除 global 重复行，保留项目差异。
#[tokio::test]
async fn two_phase_reconcile_moves_project_authority_out_of_global() {
    let project = ComponentId {
        kind: ComponentKind::SystemPromptOverlay,
        name: ComponentName::new("0123456789abcdef"),
    };
    let global = tool_guidance();
    let project_body = "# Project\n- aemeath cargo test 输出必须先按 crate 过滤再读取。\n";
    let mut registry = FakeRegistry::with_entries(vec![
        (
            global.clone(),
            body_content("# Tool Guidance\n- aemeath cargo test 输出必须先按 crate 过滤再读取。\n"),
        ),
        (project.clone(), body_content(project_body)),
    ]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let llm = TwoPhaseLlm::new(vec![
        r#"{"verdicts":[{"index":0,"verdict":"duplicate","authority":"project","reason":"规则含项目专属命令"}]}"#.to_string(),
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let outcome = crate::domain::improvement::run_two_phase_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        EvidenceBundle {
            sessions_covered: 1,
            signals: Vec::new(),
        },
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

    assert_eq!(outcome.outcome.summary, "无候选组件：仅观察");
    assert_eq!(
        registry
            .content_of(&global)
            .map(|content| content.body.clone()),
        Some("# Tool Guidance\n".to_string())
    );
    assert_eq!(
        registry
            .content_of(&project)
            .map(|content| content.body.clone()),
        Some(project_body.to_string())
    );
}

/// 无重复 pair 但 global 规则的证据只来自单项目且语义项目专属时，
/// 审计 MUST 把该规则下沉到对应 project overlay。
#[tokio::test]
async fn two_phase_moves_single_project_global_rule_to_project_overlay() {
    let project = ComponentId {
        kind: ComponentKind::SystemPromptOverlay,
        name: ComponentName::new("0123456789abcdef"),
    };
    let global = tool_guidance();
    let global_rule = "aemeath cargo test 输出必须先按 crate 过滤再读取。";
    let mut registry = FakeRegistry::with_entries(vec![
        (
            global.clone(),
            body_content(&format!("# Tool Guidance\n- {global_rule}\n")),
        ),
        (project.clone(), body_content("# Project\n")),
    ]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let historical_lesson = crate::domain::lesson::LessonEntry::new(
        "lesson-single-project".to_string(),
        "tool-guidance",
        "单项目 cargo test 输出规则",
        vec![crate::domain::lesson::EvidenceBinding {
            run_ref: "cycle-old".to_string(),
            signal: SignalKind::OversizedResult,
            excerpt: "cargo test 输出超长".to_string(),
            signal_id: "oversized_result:Bash:cargo-test".to_string(),
            source: Some(EvidenceSourceRef {
                session_id: "0123456789abcdef/s1.dataset".to_string(),
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
            }),
        }],
        1,
    )
    .expect("lesson");
    lesson_store
        .append(&historical_lesson)
        .expect("append lesson");
    let llm = TwoPhaseLlm::new(vec![
        r#"{"moves":[{"index":0,"project":"0123456789abcdef","reason":"包含 aemeath 专属 cargo test 命令"}]}"#.to_string(),
    ]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let outcome = crate::domain::improvement::run_two_phase_cycle(
        &mut registry,
        &llm,
        &lesson_store,
        &RsiConfig::default(),
        EvidenceBundle {
            sessions_covered: 1,
            signals: Vec::new(),
        },
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

    assert_eq!(outcome.outcome.summary, "无候选组件：仅观察");
    assert_eq!(
        registry
            .content_of(&global)
            .map(|content| content.body.clone()),
        Some("# Tool Guidance\n".to_string())
    );
    assert_eq!(
        registry
            .content_of(&project)
            .map(|content| content.body.clone()),
        Some(format!("# Project\n- {global_rule}\n"))
    );
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
    assert!(diagnosis_prompt.contains("项目: 0123456789abcdef,fedcba9876543210"));
    assert!(diagnosis_prompt.contains("scope=global 候选只承接跨项目通用规律"));
    assert!(diagnosis_prompt.contains("项目 A 证据 NEVER 选择项目 B 候选"));
    assert!(diagnosis_prompt.contains("工具参数类问题 NEVER 复述或改写工具 schema"));
    let execution_prompt = llm.prompt(1);
    assert!(execution_prompt.contains("目标 scope：global"));
    assert!(execution_prompt.contains("正文预算：3800 tokens"));
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

/// TwoPhase 诊断 JSON 解析失败 MUST 重试一次；重试成功继续执行。
#[tokio::test]
async fn two_phase_diagnosis_parse_failure_retries_once() {
    let target = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![(target.clone(), body_content("old"))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let malformed = r#"{"decision":"diagnose","issues":[{"evidence_ref":"E0""#;
    let valid = r#"{"decision":"diagnose","reason":"Bash 连续失败","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"重复失败","direction":"收敛重试","route_reason":"工具纪律"}]}"#;
    let llm = TwoPhaseLlm::new(vec![
        malformed.to_string(),
        valid.to_string(),
        r#"{"decision":"propose","reason":"补充诊断规则","evidence_refs":["E0"],"body":"Bash 失败后 MUST 先诊断。"}"#.to_string(),
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
    .expect("诊断重试成功后周期 MUST 继续");

    assert_eq!(outcome.outcome.summary, "采纳 1 / 无变更 0 / 作废 0");
    assert_eq!(llm.call_count(), 3);
    let retry_prompt = llm.prompt(1);
    assert!(retry_prompt.contains("诊断应答解析失败"));
    assert!(retry_prompt.contains("最多输出 8 条 issues"));
    assert!(retry_prompt.contains("每个字符串字段不超过 80 个字符"));
}

/// 诊断解析重试仍失败：MUST 显式失败，NEVER 当作 no_issue/空结果收口。
#[tokio::test]
async fn two_phase_diagnosis_parse_failure_fails_after_one_retry() {
    let target = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![(target.clone(), body_content("old"))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let malformed = r#"{"decision":"diagnose","issues":[{"evidence_ref":"E0""#;
    let llm = TwoPhaseLlm::new(vec![malformed.to_string(), malformed.to_string()]);
    let mut history = crate::domain::strength::SignalHistory::default();
    let result = crate::domain::improvement::run_two_phase_cycle(
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
    .await;
    let error = match result {
        Ok(_) => panic!("连续解析失败 MUST 显式失败"),
        Err(error) => error,
    };
    assert!(error.message().contains("诊断应答解析失败"));
    assert_eq!(
        registry
            .content_of(&target)
            .map(|content| content.body.clone()),
        Some("old".to_string())
    );
    assert!(lesson_store.load_active().expect("lessons").is_empty());
}

/// TwoPhase 执行 JSON 解析失败 MUST 重试一次；重试成功采用重试稿。
#[tokio::test]
async fn two_phase_execution_parse_failure_retries_once() {
    let target = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![(target.clone(), body_content("old"))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let malformed = r#"{"decision":"propose","reason":"截断","evidence_refs":["E0"],"body":"开头"#;
    let valid = r#"{"decision":"propose","reason":"完整规则","evidence_refs":["E0"],"body":"Bash 失败后 MUST 先诊断。"}"#;
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Bash 连续失败","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"重复失败","direction":"收敛重试","route_reason":"工具纪律"}]}"#.to_string(),
        malformed.to_string(),
        valid.to_string(),
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
    .expect("执行重试成功后周期 MUST 继续");
    assert_eq!(outcome.outcome.summary, "采纳 1 / 无变更 0 / 作废 0");
    let retry_prompt = llm.prompt(2);
    assert!(retry_prompt.contains("执行应答解析失败"));
    assert!(retry_prompt.contains("紧凑 JSON"));
    assert!(retry_prompt.contains("body 不超过 3800 tokens"));
    assert_eq!(
        registry
            .content_of(&target)
            .map(|content| content.body.clone()),
        Some("Bash 失败后 MUST 先诊断。".to_string())
    );
}

/// 执行解析重试仍失败：该任务作废且零写入，NEVER 硬截断采用半截 JSON。
#[tokio::test]
async fn two_phase_execution_parse_failure_discards_after_retry() {
    let target = tool_guidance();
    let mut registry = FakeRegistry::with_entries(vec![(target.clone(), body_content("old"))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let malformed = r#"{"decision":"propose","reason":"截断","evidence_refs":["E0"],"body":"开头"#;
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Bash 连续失败","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"重复失败","direction":"收敛重试","route_reason":"工具纪律"}]}"#.to_string(),
        malformed.to_string(),
        malformed.to_string(),
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
    .expect("单任务作废不影响周期收口");
    assert_eq!(outcome.outcome.summary, "采纳 0 / 无变更 0 / 作废 1");
    assert_eq!(
        registry
            .content_of(&target)
            .map(|content| content.body.clone()),
        Some("old".to_string())
    );
    assert!(lesson_store.load_active().expect("lessons").is_empty());
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
        "body": "长".repeat(6400),
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
        "body": "长".repeat(6400),
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

/// 既有组件正文已超预算时，执行 prompt MUST 把「语义压缩」作为第一任务，
/// NEVER 要求模型携带超长原文逐字重写后等待二次压缩。
#[tokio::test]
async fn two_phase_oversized_existing_body_gets_compaction_first_prompt() {
    let target = tool_guidance();
    let mut registry =
        FakeRegistry::with_entries(vec![(target.clone(), body_content(&"长".repeat(6400)))]);
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = LessonStore::new(lesson_dir.path().join("library.jsonl"));
    let response = serde_json::json!({
        "decision": "propose",
        "reason": "压缩既有正文并补齐工具纪律",
        "evidence_refs": ["E0"],
        "body": "Bash 失败后 MUST 先诊断；无新信息 NEVER 原样重试。",
    })
    .to_string();
    let llm = TwoPhaseLlm::new(vec![
        r#"{"decision":"diagnose","reason":"Bash 连续失败","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"重复失败","direction":"收敛规则","route_reason":"工具纪律"}]}"#.to_string(),
        response,
        r#"{"decision":"no_change","reason":"单块无冲突"}"#.to_string(),
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
    let execution_prompt = llm.prompt(1);
    assert!(execution_prompt.contains("当前正文已超过预算"));
    assert!(execution_prompt.contains("第一任务是语义压缩"));
    assert!(execution_prompt.contains("合并重复规则"));
    assert!(!execution_prompt.contains("正文必须保留与任务无关的既有规则"));
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
