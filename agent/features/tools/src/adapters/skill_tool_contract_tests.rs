//! Contract tests for the stable dynamic Skill Tool (Issue #1438).

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;

use crate::composition::{wire_builtin_catalog_execution, wire_skills};
use crate::domain::memory_source::MemoryPortSource;
use crate::domain::{
    CancellationSignal, ExecutionScope, FixedGuidance, FixedPlanMode, MutexReadSet,
    RegistryScopeName, SkillLoadDecision, SkillLoadMutation, SkillLoadScope, SkillLoadStateError,
    SkillLoadStatePort, SkillQuerySnapshot, ToolExecutionContext, ToolExecutionPorts,
    ToolInvocation, ToolName, ToolProfileName, WorkspaceReadAccess,
};

struct NeverCancelled;
#[async_trait]
impl CancellationSignal for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
    async fn cancelled(&self) {
        std::future::pending::<()>().await
    }
    fn child_signal(&self) -> Arc<dyn CancellationSignal> {
        Arc::new(Self)
    }
}

struct FixedSkillState(Result<SkillLoadDecision, SkillLoadStateError>);

#[async_trait]
impl SkillLoadStatePort for FixedSkillState {
    async fn compare_and_record(
        &self,
        _mutation: SkillLoadMutation,
    ) -> Result<SkillLoadDecision, SkillLoadStateError> {
        self.0.clone()
    }
}

struct FreshSkillState;

#[async_trait]
impl SkillLoadStatePort for FreshSkillState {
    async fn compare_and_record(
        &self,
        _mutation: SkillLoadMutation,
    ) -> Result<SkillLoadDecision, SkillLoadStateError> {
        Ok(SkillLoadDecision::Fresh)
    }
}

fn memory_source() -> Arc<dyn MemoryPortSource> {
    struct Source;
    impl MemoryPortSource for Source {
        fn current(&self) -> Arc<dyn memory::MemoryPort> {
            Arc::new(memory::NoOpMemory)
        }
    }
    Arc::new(Source)
}

fn write_skill(root: &std::path::Path, name: &str, body: &str) {
    let dir = root.join(".agents/skills").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: test\n---\n{body}"),
    )
    .unwrap();
}

#[tokio::test]
async fn main_and_sub_catalog_publish_exact_skill_schema_and_execute_body() {
    let temp = tempfile::tempdir().unwrap();
    write_skill(temp.path(), "commit", "SKILL_BODY_SENTINEL");
    let workspace = project::wire_production_workspace(temp.path().to_path_buf())
        .unwrap()
        .into_views();
    let skill = wire_skills();
    let wiring = wire_builtin_catalog_execution(
        Arc::new(task::TaskStore::new()),
        memory_source(),
        workspace.control(),
        skill.loader(),
    )
    .unwrap();

    let expected = json!({
        "type": "object",
        "properties": { "skill": { "type": "string", "minLength": 1 } },
        "required": ["skill"],
        "additionalProperties": false
    });
    for (scope, profile) in [("main", "main-full"), ("sub-agent", "sub-agent-restricted")] {
        let snapshot = wiring
            .catalog()
            .snapshot(
                &RegistryScopeName::new(scope),
                &ToolProfileName::new(profile),
            )
            .unwrap();
        let descriptor = snapshot
            .find(&ToolName::new("Skill"))
            .expect("Skill catalog entry");
        assert_eq!(descriptor.input_schema, expected);
        let schema_text = descriptor.input_schema.to_string();
        assert!(!schema_text.contains("args"));
        assert!(!schema_text.contains("arguments"));
        assert!(!schema_text.contains("content"));

        let execution_scope = ExecutionScope::builder(
            format!("{scope}-run"),
            workspace.read().workspace_id(),
            temp.path().to_path_buf(),
        )
        .registry_scope(RegistryScopeName::new(scope))
        .profile(ToolProfileName::new(profile))
        .build();
        let ctx = ToolExecutionContext::new(
            execution_scope.clone(),
            ToolExecutionPorts::new(
                Arc::new(NeverCancelled),
                WorkspaceReadAccess::new(workspace.read()),
                Arc::new(MutexReadSet(Arc::new(Mutex::new(HashSet::new())))),
                Arc::new(FixedPlanMode(None)),
                Arc::new(memory::NoOpMemory),
                Arc::new(FixedGuidance {
                    language: "en".into(),
                }),
            )
            .with_skill_query(SkillQuerySnapshot {
                extra_dirs: Vec::<PathBuf>::new(),
                available_tools: BTreeSet::from(["Skill".to_string()]),
            })
            .with_memory_context(Some("session".to_string()), None)
            .with_skill_load_state(
                if scope == "main" {
                    SkillLoadScope::main()
                } else {
                    SkillLoadScope::subagent("sub-agent-instance").unwrap()
                },
                Arc::new(FreshSkillState),
            ),
        );
        let outcome = wiring
            .execution()
            .execute(
                ToolInvocation::new("Skill", json!({"skill": "commit"}), execution_scope),
                &ctx,
            )
            .await;
        match outcome {
            crate::domain::ToolExecutionOutcome::Success(success) => {
                assert_eq!(success.content[0].text, "SKILL_BODY_SENTINEL");
                let data = success.data.expect("structured Skill load metadata");
                assert_eq!(data["name"], "commit");
                assert!(data.get("content").is_none());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn skill_state_decision_controls_body_without_leaking_on_failure() {
    for (decision, expected_body, expected_hint, expect_success) in [
        (Ok(SkillLoadDecision::Updated), true, false, true),
        (Ok(SkillLoadDecision::AlreadyLoaded), false, true, true),
        (
            Err(SkillLoadStateError::Storage("disk full".to_string())),
            false,
            false,
            false,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        write_skill(temp.path(), "review", "BODY_SENTINEL");
        let workspace = project::wire_production_workspace(temp.path().to_path_buf())
            .unwrap()
            .into_views();
        let skill = wire_skills();
        let wiring = wire_builtin_catalog_execution(
            Arc::new(task::TaskStore::new()),
            memory_source(),
            workspace.control(),
            skill.loader(),
        )
        .unwrap();
        let scope = ExecutionScope::builder(
            "run",
            workspace.read().workspace_id(),
            temp.path().to_path_buf(),
        )
        .build();
        let ctx = ToolExecutionContext::new(
            scope.clone(),
            ToolExecutionPorts::new(
                Arc::new(NeverCancelled),
                WorkspaceReadAccess::new(workspace.read()),
                Arc::new(MutexReadSet(Arc::new(Mutex::new(HashSet::new())))),
                Arc::new(FixedPlanMode(None)),
                Arc::new(memory::NoOpMemory),
                Arc::new(FixedGuidance {
                    language: "zh".into(),
                }),
            )
            .with_memory_context(Some("session".to_string()), None)
            .with_skill_load_state(SkillLoadScope::main(), Arc::new(FixedSkillState(decision))),
        );
        let outcome = wiring
            .execution()
            .execute(
                ToolInvocation::new("Skill", json!({"skill": "review"}), scope),
                &ctx,
            )
            .await;
        let text = match &outcome {
            crate::domain::ToolExecutionOutcome::Success(success) => {
                assert!(expect_success);
                success.content[0].text.as_str()
            }
            crate::domain::ToolExecutionOutcome::Failure(failure) => {
                assert!(!expect_success);
                failure.content[0].text.as_str()
            }
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert_eq!(text.contains("BODY_SENTINEL"), expected_body);
        assert_eq!(text.contains("已加载"), expected_hint);
    }
}

#[tokio::test]
async fn deleted_skill_returns_failure_without_panicking() {
    let temp = tempfile::tempdir().unwrap();
    write_skill(temp.path(), "gone", "body");
    let workspace = project::wire_production_workspace(temp.path().to_path_buf())
        .unwrap()
        .into_views();
    let skill = wire_skills();
    let wiring = wire_builtin_catalog_execution(
        Arc::new(task::TaskStore::new()),
        memory_source(),
        workspace.control(),
        skill.loader(),
    )
    .unwrap();
    std::fs::remove_dir_all(temp.path().join(".agents/skills/gone")).unwrap();
    let scope = ExecutionScope::builder(
        "deleted",
        workspace.read().workspace_id(),
        temp.path().to_path_buf(),
    )
    .registry_scope(RegistryScopeName::new("main"))
    .profile(ToolProfileName::new("main-full"))
    .build();
    let ctx = ToolExecutionContext::new(
        scope.clone(),
        ToolExecutionPorts::new(
            Arc::new(NeverCancelled),
            WorkspaceReadAccess::new(workspace.read()),
            Arc::new(MutexReadSet(Arc::new(Mutex::new(HashSet::new())))),
            Arc::new(FixedPlanMode(None)),
            Arc::new(memory::NoOpMemory),
            Arc::new(FixedGuidance {
                language: "en".into(),
            }),
        ),
    );
    let outcome = wiring
        .execution()
        .execute(
            ToolInvocation::new("Skill", json!({"skill": "gone"}), scope),
            &ctx,
        )
        .await;
    assert!(matches!(
        outcome,
        crate::domain::ToolExecutionOutcome::Failure(_)
    ));
}
