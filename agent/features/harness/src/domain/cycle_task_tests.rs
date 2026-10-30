use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::errors::HarnessError;
use crate::domain::evidence::{
    EvidenceBundle, EvidenceSignal, EvidenceSourceRef, SessionContextExcerpt, SignalKind,
};
use crate::domain::improvement::RsiLlmPort;
use crate::domain::lesson::{EvidenceBinding, LessonEntry, LessonStore};
use crate::domain::stream::{EvidenceBlockPlan, RunRef};
use crate::domain::stream_port::FakeEvidenceStream;
use crate::domain::task::RsiTask;
use crate::domain::test_support::FakeRegistry;
use async_trait::async_trait;
use share::config::RsiConfig;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

fn component_id(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::ToolGuidance,
        name: ComponentName::new(name),
    }
}

fn body_content(body: &str) -> ComponentContent {
    ComponentContent {
        frontmatter: None,
        body: body.to_string(),
    }
}

struct PromptRecordingLlm {
    responses: Vec<String>,
    calls: std::sync::atomic::AtomicUsize,
    prompts: Mutex<Vec<String>>,
}

impl PromptRecordingLlm {
    fn new(responses: Vec<String>) -> Arc<Self> {
        Arc::new(Self {
            responses,
            calls: std::sync::atomic::AtomicUsize::new(0),
            prompts: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl RsiLlmPort for PromptRecordingLlm {
    async fn complete(&self, prompt: &str) -> Result<String, HarnessError> {
        self.prompts
            .lock()
            .expect("prompts lock")
            .push(prompt.to_string());
        let index = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.responses
            .get(index)
            .cloned()
            .ok_or_else(|| HarnessError::unavailable(crate::constants::ERROR_DOMAIN, "桩耗尽"))
    }
}

/// dry-run 必须跑完整链路：改进块虚拟采纳 → Compose 看到叠加后的虚拟终态；
/// 当前测试先证明既有实现只调用改进块、不进入 Compose。
#[tokio::test]
async fn dry_run_runs_compose_against_virtual_final_components() {
    let registry: Box<dyn crate::application::ComponentRegistry + Send> = Box::new(
        FakeRegistry::with_entries(vec![(component_id("alpha"), body_content("old"))]),
    );
    let registry = Arc::new(AsyncMutex::new(registry));
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = Arc::new(LessonStore::new(lesson_dir.path().join("library.jsonl")));
    let lesson_store_for_assert = Arc::clone(&lesson_store);
    let source = EvidenceSourceRef {
        session_id: "s1.dataset".into(),
        run_id: "run-1".into(),
        step_id: "step-2".into(),
    };
    let historical_lesson = LessonEntry::new(
        "lesson-old".to_string(),
        "skill",
        "编辑文件前应读取目标内容，确认上下文后再落笔".to_string(),
        vec![EvidenceBinding {
            run_ref: "cycle-old".into(),
            signal: SignalKind::RepeatedFailure,
            excerpt: "旧证据".into(),
            signal_id: "repeated_failure:Edit:dry-run".into(),
            source: Some(source.clone()),
        }],
        1,
    )
    .expect("lesson");
    lesson_store
        .append(&historical_lesson)
        .expect("append lesson");

    let stream = FakeEvidenceStream::default();
    stream.plans.lock().expect("plans").push(EvidenceBlockPlan {
        runs: vec![RunRef {
            session_key: "s1.dataset".into(),
            run_id: "run-1".into(),
        }],
    });
    *stream.evidence.lock().expect("evidence") = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RepeatedFailure,
            signal_id: "repeated_failure:Edit:dry-run".into(),
            summary: "Edit 重复失败 ×2".into(),
            excerpt: Some("old_string mismatch".into()),
            occurrences: 2,
            sources: vec![EvidenceSourceRef {
                session_id: "s1.dataset".into(),
                run_id: "run-1".into(),
                step_id: "step-1".into(),
            }],
        }],
    };
    stream.contexts.lock().expect("contexts").extend([
        SessionContextExcerpt {
            source: EvidenceSourceRef {
                session_id: "s1.dataset".into(),
                run_id: "run-1".into(),
                step_id: "step-1".into(),
            },
            text: "当前提案上下文".into(),
        },
        SessionContextExcerpt {
            source,
            text: "历史 lesson 上下文".into(),
        },
    ]);

    let llm = PromptRecordingLlm::new(vec![
        r#"{"decision":"propose","reason":"编辑前必须先读取文件内容并确认上下文","signal_ids":["repeated_failure:Edit:dry-run"],"change_set":{"edits":[{"action":"upsert","id":{"kind":"tool-guidance","name":"alpha"},"content":{"frontmatter":null,"body":"virtual-final-body"}}]}}"#.to_string(),
        r#"{"decision":"no_change","reason":"上下文不同"}"#.to_string(),
    ]);
    let mut task = crate::domain::cycle_task::HarnessRsiCycleTask::new(
        registry,
        llm.clone(),
        lesson_store,
        RsiConfig {
            chunk_max_components: 4,
            ..RsiConfig::default()
        },
    )
    .with_dry_run()
    .with_evidence_stream(Box::new(stream));
    let outcome = task.run_cycle().await.expect("dry-run cycle");
    assert!(outcome.summary.contains("Compose"), "{}", outcome.summary);
    assert!(
        !outcome.summary.contains("Compose：（dry-run）Compose"),
        "dry-run 报告不得重复 Compose 前缀：{}",
        outcome.summary
    );
    assert_eq!(
        lesson_store_for_assert
            .load_active()
            .expect("lessons")
            .len(),
        1,
        "dry-run Compose NEVER 追加或退役 lesson"
    );

    let prompts = llm.prompts.lock().expect("prompts").clone();
    assert_eq!(
        prompts.len(),
        2,
        "dry-run 必须依次执行改进块与 Compose：{prompts:?}"
    );
    let compose_prompt = &prompts[1];
    assert!(
        compose_prompt.contains("virtual-final-body"),
        "Compose 必须看到虚拟叠加终态：{compose_prompt}"
    );
    assert!(
        compose_prompt.contains("当前提案上下文") && compose_prompt.contains("历史 lesson 上下文"),
        "Compose 必须看到双方来源上下文：{compose_prompt}"
    );
}

/// 端到端 dry-run 用例：两个证据块采纳同一场景的相反规则，Compose 必须
/// 产出仲裁修正并进入最终预览；这证明 dry-run 不止“调用了 Compose”，
/// 而是真的能走到「冲突 → 修正 → 预览」的终点。
#[tokio::test]
async fn dry_run_compose_resolves_conflicting_adoptions_to_final_preview() {
    let guardrails = ComponentId {
        kind: ComponentKind::Guardrails,
        name: ComponentName::new("default"),
    };
    let tool_guidance = ComponentId {
        kind: ComponentKind::ToolGuidance,
        name: ComponentName::new("default"),
    };
    let registry: Box<dyn crate::application::ComponentRegistry + Send> =
        Box::new(FakeRegistry::with_entries(vec![
            (
                guardrails.clone(),
                body_content("# Guardrails\n\n- 保持稳定。"),
            ),
            (
                tool_guidance.clone(),
                body_content("# Tool Guidance\n\n- 保持稳定。"),
            ),
        ]));
    let registry = Arc::new(AsyncMutex::new(registry));
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = Arc::new(LessonStore::new(lesson_dir.path().join("library.jsonl")));

    let stream = FakeEvidenceStream::default();
    for run_id in ["run-1", "run-2"] {
        stream.plans.lock().expect("plans").push(EvidenceBlockPlan {
            runs: vec![RunRef {
                session_key: "s1.dataset".into(),
                run_id: run_id.into(),
            }],
        });
    }
    let source = EvidenceSourceRef {
        session_id: "s1.dataset".into(),
        run_id: "run-1".into(),
        step_id: "step-1".into(),
    };
    *stream.evidence.lock().expect("evidence") = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RepeatedFailure,
            signal_id: "repeated_failure:Bash:unchanged-retry".into(),
            summary: "Bash 同参数失败后重复调用 ×2".into(),
            excerpt: Some("exit code 1: unchanged retry".into()),
            occurrences: 2,
            sources: vec![source.clone()],
        }],
    };
    stream
        .contexts
        .lock()
        .expect("contexts")
        .push(SessionContextExcerpt {
            source,
            text: "两个 run 都在 Bash 失败后未修改命令即重试".into(),
        });

    let conflict_rule = "Bash 失败后必须立即原样重试一次。";
    let opposite_rule = "Bash 失败后禁止原样重试；必须先修改命令或代码。";
    let resolved_rule = "Bash 失败后禁止原样重试；先读取 stderr，修正命令或代码后再执行。";
    let llm = PromptRecordingLlm::new(vec![
        format!(
            r#"{{"decision":"propose","reason":"第一块证据要求失败后重试","signal_ids":["repeated_failure:Bash:unchanged-retry"],"change_set":{{"edits":[{{"action":"upsert","id":{{"kind":"guardrails","name":"default"}},"content":{{"frontmatter":null,"body":"Guardrails {conflict_rule}"}}}}]}}}}"#
        ),
        format!(
            r#"{{"decision":"propose","reason":"第二块证据要求禁止机械重试","signal_ids":["repeated_failure:Bash:unchanged-retry"],"change_set":{{"edits":[{{"action":"upsert","id":{{"kind":"tool-guidance","name":"default"}},"content":{{"frontmatter":null,"body":"Tool Guidance {opposite_rule}"}}}}]}}}}"#
        ),
        format!(
            r#"{{"decision":"propose","reason":"两条采纳在相同 Bash 失败场景下给出相反指令，保留证据支持的禁止重试规则","signal_ids":[],"change_set":{{"edits":[{{"action":"upsert","id":{{"kind":"guardrails","name":"default"}},"content":{{"frontmatter":null,"body":"Guardrails {resolved_rule}"}}}}]}}}}"#
        ),
    ]);
    let mut task = crate::domain::cycle_task::HarnessRsiCycleTask::new(
        registry,
        llm.clone(),
        lesson_store,
        RsiConfig {
            chunk_max_components: 4,
            ..RsiConfig::default()
        },
    )
    .with_dry_run()
    .with_evidence_stream(Box::new(stream));
    let outcome = task.run_cycle().await.expect("dry-run cycle");

    assert!(
        outcome
            .summary
            .contains("（dry-run）Compose：仲裁修正（将采纳）"),
        "Compose 必须走到修正终态：{}",
        outcome.summary
    );
    assert!(
        outcome
            .summary
            .contains("[dry-run][Compose] upsert guardrails/default"),
        "最终预览必须包含 Compose 修正：{}",
        outcome.summary
    );
    assert!(
        outcome.summary.contains(resolved_rule),
        "最终预览必须保留修正后的规则：{}",
        outcome.summary
    );
    let compose_marker = outcome
        .summary
        .find("[dry-run][Compose]")
        .expect("Compose 预览标记");
    let resolved_position = outcome.summary.find(resolved_rule).expect("修正规则");
    assert!(
        compose_marker < resolved_position,
        "修正规则必须出现在 Compose 预览之后：{}",
        outcome.summary
    );

    let prompts = llm.prompts.lock().expect("prompts").clone();
    assert_eq!(prompts.len(), 3, "两块提案 + Compose");
    let compose_prompt = &prompts[2];
    assert!(
        compose_prompt.contains(conflict_rule) && compose_prompt.contains(opposite_rule),
        "Compose 必须同时看到两条虚拟采纳后的冲突终态：{compose_prompt}"
    );
}

#[tokio::test]
async fn two_phase_cycle_uses_router_and_fixed_execution() {
    let registry: Box<dyn crate::application::ComponentRegistry + Send> = Box::new(
        FakeRegistry::with_entries(vec![(component_id("default"), body_content("old"))]),
    );
    let registry = Arc::new(AsyncMutex::new(registry));
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = Arc::new(LessonStore::new(lesson_dir.path().join("library.jsonl")));
    let stream = FakeEvidenceStream::default();
    stream.plans.lock().expect("plans").push(EvidenceBlockPlan {
        runs: vec![RunRef {
            session_key: "s1.dataset".into(),
            run_id: "run-1".into(),
        }],
    });
    *stream.evidence.lock().expect("evidence") = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RepeatedFailure,
            signal_id: "repeated_failure:Bash:test-failed".into(),
            summary: "Bash 连续失败 ×3".into(),
            excerpt: Some("exit code 1".into()),
            occurrences: 3,
            sources: Vec::new(),
        }],
    };
    let llm = PromptRecordingLlm::new(vec![
        r#"{"decision":"diagnose","reason":"工具纪律","issues":[{"evidence_ref":"E0","component_candidate":0,"problem":"Bash 失败后未诊断","direction":"补充规则","route_reason":"通用工具纪律"}]}"#.to_string(),
        r#"{"decision":"propose","reason":"补充规则","evidence_refs":["E0"],"body":"new body"}"#.to_string(),
        r#"{"decision":"no_change","reason":"无冲突"}"#.to_string(),
    ]);
    let mut task = crate::domain::cycle_task::HarnessRsiCycleTask::new(
        registry,
        llm.clone(),
        lesson_store,
        RsiConfig {
            execution_mode: share::config::RsiExecutionMode::TwoPhase,
            ..RsiConfig::default()
        },
    )
    .with_dry_run()
    .with_evidence_stream(Box::new(stream));
    let outcome = task.run_cycle().await.expect("two-phase cycle");
    assert!(
        outcome.summary.contains("TwoPhase") && outcome.summary.contains("Compose"),
        "{}",
        outcome.summary
    );
    let prompts = llm.prompts.lock().expect("prompts").clone();
    assert_eq!(prompts.len(), 3, "诊断 + 固定执行 + Compose");
    assert!(
        prompts[0].contains("C0=tool-guidance/default"),
        "诊断 prompt 必须只提供路由候选：{}",
        prompts[0]
    );
    assert!(
        prompts[1].contains("NEVER 输出组件 id"),
        "执行 prompt 必须固定组件：{}",
        prompts[1]
    );
}

/// 完整周期：Compose 必须使用采纳前 Active lesson 快照，避免历史经验
/// 被本周期 Block 退役，也避免当前 proposal 与自身派生 lesson 配对。
#[tokio::test]
async fn compose_uses_active_lesson_snapshot_before_block_adoptions() {
    let registry: Box<dyn crate::application::ComponentRegistry + Send> = Box::new(
        FakeRegistry::with_entries(vec![(component_id("alpha"), body_content("old"))]),
    );
    let registry = Arc::new(AsyncMutex::new(registry));
    let lesson_dir = tempfile::tempdir().expect("tempdir");
    let lesson_store = Arc::new(LessonStore::new(lesson_dir.path().join("library.jsonl")));
    let source = EvidenceSourceRef {
        session_id: "s1.dataset".into(),
        run_id: "run-1".into(),
        step_id: "step-2".into(),
    };
    let historical_lesson = LessonEntry::new(
        "lesson-old".to_string(),
        "skill",
        "编辑文件前应读取目标内容，确认上下文后再落笔".to_string(),
        vec![EvidenceBinding {
            run_ref: "cycle-old".into(),
            signal: SignalKind::RepeatedFailure,
            excerpt: "旧证据".into(),
            signal_id: "repeated_failure:Edit:old-signal".into(),
            source: Some(source.clone()),
        }],
        1,
    )
    .expect("lesson");
    lesson_store
        .append(&historical_lesson)
        .expect("append lesson");

    let stream = FakeEvidenceStream::default();
    stream.plans.lock().expect("plans").push(EvidenceBlockPlan {
        runs: vec![RunRef {
            session_key: "s1.dataset".into(),
            run_id: "run-1".into(),
        }],
    });
    *stream.evidence.lock().expect("evidence") = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RepeatedFailure,
            signal_id: "repeated_failure:Edit:new-signal".into(),
            summary: "Edit 重复失败 ×2".into(),
            excerpt: Some("old_string mismatch".into()),
            occurrences: 2,
            sources: vec![EvidenceSourceRef {
                session_id: "s1.dataset".into(),
                run_id: "run-1".into(),
                step_id: "step-1".into(),
            }],
        }],
    };
    stream.contexts.lock().expect("contexts").extend([
        SessionContextExcerpt {
            source: EvidenceSourceRef {
                session_id: "s1.dataset".into(),
                run_id: "run-1".into(),
                step_id: "step-1".into(),
            },
            text: "当前提案上下文".into(),
        },
        SessionContextExcerpt {
            source,
            text: "历史 lesson 上下文".into(),
        },
    ]);

    let llm = PromptRecordingLlm::new(vec![
        r#"{"decision":"propose","reason":"编辑前必须先读取文件内容并确认上下文","signal_ids":["repeated_failure:Edit:new-signal"],"change_set":{"edits":[{"action":"upsert","id":{"kind":"tool-guidance","name":"alpha"},"content":{"frontmatter":null,"body":"new"}}]}}"#.to_string(),
        r#"{"decision":"no_change","reason":"上下文不同"}"#.to_string(),
    ]);
    let mut task = crate::domain::cycle_task::HarnessRsiCycleTask::new(
        registry,
        llm.clone(),
        lesson_store,
        RsiConfig {
            chunk_max_components: 4,
            ..RsiConfig::default()
        },
    )
    .with_evidence_stream(Box::new(stream));
    let outcome = task.run_cycle().await.expect("cycle");
    assert!(outcome.summary.contains("Compose"), "{}", outcome.summary);

    let prompts = llm.prompts.lock().expect("prompts").clone();
    let compose_prompt = prompts.last().expect("Block + Compose 两次 LLM 调用");
    assert!(
        compose_prompt.contains("lesson-old"),
        "Compose 必须看到采纳前历史 lesson：{compose_prompt}"
    );
    assert!(
        compose_prompt.contains("历史 lesson 上下文"),
        "历史 lesson 来源上下文必须进入 Compose：{compose_prompt}"
    );
    assert!(
        compose_prompt.contains("当前提案上下文"),
        "当前提案来源上下文必须进入 Compose：{compose_prompt}"
    );
    assert!(
        !compose_prompt.contains("cycle-"),
        "Compose Active lesson 快照不得混入本周期自身派生 lesson：{compose_prompt}"
    );
}
