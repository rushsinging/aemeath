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
        r#"{"decision":"propose","reason":"编辑前必须先读取文件内容并确认上下文","signal_ids":["repeated_failure:Edit:dry-run"],"change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"alpha"},"content":{"frontmatter":null,"body":"virtual-final-body"}}]}}"#.to_string(),
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
        r#"{"decision":"propose","reason":"编辑前必须先读取文件内容并确认上下文","signal_ids":["repeated_failure:Edit:new-signal"],"change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"alpha"},"content":{"frontmatter":null,"body":"new"}}]}}"#.to_string(),
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
