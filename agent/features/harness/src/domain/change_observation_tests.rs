use super::*;
use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::evidence::{EvidenceBundle, EvidenceSignal, SignalKind};
use crate::domain::test_support::FakeRegistry;
use crate::domain::versioning::ComponentVersionStore;

fn id() -> ComponentId {
    ComponentId {
        kind: ComponentKind::Guardrails,
        name: ComponentName::new("default"),
    }
}

fn content(body: &str) -> ComponentContent {
    ComponentContent {
        frontmatter: None,
        body: body.to_string(),
    }
}

#[test]
fn write_then_target_reappears_rolls_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    let mut registry =
        FakeRegistry::with_entries(vec![(id(), content("# Guardrails\n\n- old rule\n"))]);
    versions
        .capture(&registry, &id(), "cycle-1", "upsert")
        .expect("capture");
    let target = "retry_pattern:Bash:cargo test".to_string();
    let observation = ChangeObservation::new_applied(
        id().to_string(),
        "cycle-1".to_string(),
        body_hash("# Guardrails\n\n- old rule\n"),
        body_hash("# Guardrails\n\n- new rule\n"),
        vec![target.clone()],
        Vec::new(),
        vec!["run-old".to_string()],
    );
    store.append(&observation).expect("append");
    registry
        .write(&id(), &content("# Guardrails\n\n- new rule\n"))
        .expect("write");

    let evidence = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RetryPattern,
            signal_id: target,
            summary: "same command failed".to_string(),
            excerpt: None,
            occurrences: 1,
            sources: vec![crate::domain::evidence::EvidenceSourceRef {
                session_id: "proj/s.dataset".to_string(),
                run_id: "run-new".to_string(),
                step_id: "step-1".to_string(),
            }],
        }],
    };
    let summaries = evaluate_and_rollback(&store, &mut registry, &versions, &evidence, None);
    assert!(
        summaries.iter().any(|line| line.contains("自动回滚")),
        "{summaries:?}"
    );
    assert_eq!(
        registry.content_of(&id()).unwrap().body,
        "# Guardrails\n\n- old rule\n"
    );
    assert_eq!(
        store.load().unwrap()[0].status,
        ObservationStatus::RolledBack
    );
}

#[test]
fn external_modification_marks_stale_without_overwrite() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    let mut registry =
        FakeRegistry::with_entries(vec![(id(), content("# Guardrails\n\n- external rule\n"))]);
    let target = "retry_pattern:Bash:cargo test".to_string();
    store
        .append(&ChangeObservation::new_applied(
            id().to_string(),
            "cycle-1".to_string(),
            body_hash("old"),
            body_hash("new"),
            vec![target.clone()],
            Vec::new(),
            vec!["run-old".to_string()],
        ))
        .expect("append");
    let evidence = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RetryPattern,
            signal_id: target,
            summary: "same command failed".to_string(),
            excerpt: None,
            occurrences: 1,
            sources: vec![crate::domain::evidence::EvidenceSourceRef {
                session_id: "proj/s.dataset".to_string(),
                run_id: "run-new".to_string(),
                step_id: "step-1".to_string(),
            }],
        }],
    };
    evaluate_and_rollback(&store, &mut registry, &versions, &evidence, None);
    assert_eq!(
        registry.content_of(&id()).unwrap().body,
        "# Guardrails\n\n- external rule\n"
    );
    assert_eq!(store.load().unwrap()[0].status, ObservationStatus::Stale);
}

#[test]
fn historical_run_does_not_count_as_reappearance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    let mut registry =
        FakeRegistry::with_entries(vec![(id(), content("# Guardrails\n\n- new rule\n"))]);
    let target = "retry_pattern:Bash:cargo test".to_string();
    store
        .append(&ChangeObservation::new_applied(
            id().to_string(),
            "cycle-1".to_string(),
            body_hash("old"),
            body_hash("# Guardrails\n\n- new rule\n"),
            vec![target.clone()],
            Vec::new(),
            vec!["run-old".to_string()],
        ))
        .expect("append");
    let evidence = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RetryPattern,
            signal_id: target,
            summary: "historical".to_string(),
            excerpt: None,
            occurrences: 1,
            sources: vec![crate::domain::evidence::EvidenceSourceRef {
                session_id: "proj/s.dataset".to_string(),
                run_id: "run-old".to_string(),
                step_id: "step-1".to_string(),
            }],
        }],
    };

    let summaries = evaluate_and_rollback(&store, &mut registry, &versions, &evidence, None);
    assert!(summaries.is_empty(), "{summaries:?}");
    assert_eq!(
        store.load().unwrap()[0].status,
        ObservationStatus::Observing
    );
}

#[test]
fn observation_stores_signal_hash_and_redacted_display() {
    let observation = ChangeObservation::new_applied(
        "tool-guidance/default".to_string(),
        "cycle-1".to_string(),
        body_hash("old"),
        body_hash("new"),
        vec!["retry_pattern:Bash:{\"command\":\"curl -H sk-secret-123\"}".to_string()],
        vec!["oversized_result:Bash".to_string()],
        vec!["run-1".to_string()],
    );
    assert_eq!(observation.target_signal_hashes.len(), 1);
    assert!(!observation.target_signal_ids[0].contains("sk-secret-123"));
    assert_eq!(observation.target_signal_ids[0], "retry_pattern:Bash");
    assert_eq!(observation.baseline_signal_ids[0], "oversized_result:Bash");
}

#[test]
fn later_write_supersedes_previous_observation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let observation = ChangeObservation::new_applied(
        "guardrails/default".to_string(),
        "cycle-1".to_string(),
        body_hash("old"),
        body_hash("new"),
        vec!["retry_pattern:Bash".to_string()],
        Vec::new(),
        vec!["run-1".to_string()],
    );
    store.append(&observation).expect("append");
    let count = store
        .supersede_component("guardrails/default", "被后续写入覆盖")
        .expect("supersede");
    assert_eq!(count, 1);
    assert_eq!(
        store.load().unwrap()[0].status,
        ObservationStatus::Superseded
    );
}

#[test]
fn newer_snapshot_supersedes_instead_of_stale() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    let mut registry = FakeRegistry::with_entries(vec![(id(), content("A"))]);
    versions
        .capture(&registry, &id(), "cycle-1", "upsert")
        .expect("capture 1");
    registry.write(&id(), &content("B")).expect("write B");
    versions
        .capture(&registry, &id(), "cycle-2", "upsert")
        .expect("capture 2");
    let target = "retry_pattern:Bash:cargo test".to_string();
    store
        .append(&ChangeObservation::new_applied(
            id().to_string(),
            "cycle-1".to_string(),
            body_hash("old"),
            body_hash("A"),
            vec![target.clone()],
            Vec::new(),
            vec!["run-old".to_string()],
        ))
        .expect("append");
    let evidence = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RetryPattern,
            signal_id: target,
            summary: "new".to_string(),
            excerpt: None,
            occurrences: 1,
            sources: vec![crate::domain::evidence::EvidenceSourceRef {
                session_id: "proj/s.dataset".to_string(),
                run_id: "run-new".to_string(),
                step_id: "step-1".to_string(),
            }],
        }],
    };

    evaluate_and_rollback(&store, &mut registry, &versions, &evidence, None);
    assert_eq!(
        store.load().unwrap()[0].status,
        ObservationStatus::Superseded
    );
    assert_eq!(registry.content_of(&id()).unwrap().body, "B");
}

#[test]
fn rollback_supersedes_component_lesson() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    let lessons = crate::domain::lesson::LessonStore::new(dir.path().join("library.jsonl"));
    let mut registry =
        FakeRegistry::with_entries(vec![(id(), content("# Guardrails\n\n- old rule\n"))]);
    versions
        .capture(&registry, &id(), "cycle-1", "upsert")
        .expect("capture");
    let target = "retry_pattern:Bash:cargo test".to_string();
    let lesson = crate::domain::lesson::LessonEntry::new(
        "cycle-1:guardrails/default".to_string(),
        "guardrails",
        "旧经验".to_string(),
        vec![crate::domain::lesson::EvidenceBinding {
            run_ref: "cycle-1".to_string(),
            signal: SignalKind::RetryPattern,
            excerpt: "e".to_string(),
            signal_id: target.clone(),
            source: None,
        }],
        1,
    )
    .expect("lesson");
    lessons.append(&lesson).expect("append lesson");
    store
        .append(&ChangeObservation::new_applied(
            id().to_string(),
            "cycle-1".to_string(),
            body_hash("# Guardrails\n\n- old rule\n"),
            body_hash("# Guardrails\n\n- new rule\n"),
            vec![target.clone()],
            Vec::new(),
            vec!["run-old".to_string()],
        ))
        .expect("append obs");
    registry
        .write(&id(), &content("# Guardrails\n\n- new rule\n"))
        .expect("write");
    let evidence = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RetryPattern,
            signal_id: target,
            summary: "new".to_string(),
            excerpt: None,
            occurrences: 1,
            sources: vec![crate::domain::evidence::EvidenceSourceRef {
                session_id: "proj/s.dataset".to_string(),
                run_id: "run-new".to_string(),
                step_id: "step-1".to_string(),
            }],
        }],
    };

    evaluate_and_rollback(&store, &mut registry, &versions, &evidence, Some(&lessons));
    let all = lessons.load_all().expect("lessons");
    assert_eq!(
        all[0].status,
        crate::domain::lesson::LessonStatus::Superseded
    );
}

#[test]
fn new_skill_without_usage_evidence_is_not_kept_or_rolled_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let observation = ChangeObservation::new_applied(
        "skill/issue-to-pr".to_string(),
        "cycle-new".to_string(),
        body_hash(""),
        body_hash("new skill"),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    store.append(&observation).expect("append");
    let mut registry = FakeRegistry::with_entries(Vec::new());
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    let evidence = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::RetryPattern,
            signal_id: "retry_pattern:Bash:cargo-test".into(),
            summary: "unrelated".into(),
            excerpt: None,
            occurrences: 1,
            sources: Vec::new(),
        }],
    };
    evaluate_and_rollback(&store, &mut registry, &versions, &evidence, None);
    let saved = store.load().expect("load");
    assert_eq!(saved[0].status, ObservationStatus::Observing);
    assert_eq!(saved[0].observation_cycles, 0);
}

#[test]
fn new_skill_load_waits_for_effect_verification() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let observation = ChangeObservation::new_applied(
        "skill/issue-to-pr".to_string(),
        "cycle-new".to_string(),
        body_hash(""),
        body_hash("new skill"),
        vec!["skill_usage:issue-to-pr".into()],
        Vec::new(),
        vec!["run-before".into()],
    );
    store.append(&observation).expect("append");
    let mut registry = FakeRegistry::with_entries(Vec::new());
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    let evidence = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::SkillUsage,
            signal_id: "skill_usage:issue-to-pr".into(),
            summary: "Skill issue-to-pr 成功加载 ×1（仅证明采用）".into(),
            excerpt: None,
            occurrences: 1,
            sources: vec![crate::domain::evidence::EvidenceSourceRef {
                session_id: "project/session.dataset".into(),
                run_id: "run-after".into(),
                step_id: "step-1".into(),
            }],
        }],
    };
    let summaries = evaluate_and_rollback(&store, &mut registry, &versions, &evidence, None);
    let saved = store.load().expect("load");
    assert_eq!(saved[0].status, ObservationStatus::Observing);
    assert!(summaries.join("\n").contains("等待使用效果验证"));
}

#[test]
fn new_skill_without_usage_remains_observing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let observation = ChangeObservation::new_applied(
        "skill/issue-to-pr".to_string(),
        "cycle-new".to_string(),
        body_hash(""),
        body_hash("new skill"),
        vec!["skill_usage:issue-to-pr".into()],
        Vec::new(),
        vec!["run-before".into()],
    );
    store.append(&observation).expect("append");
    let mut registry = FakeRegistry::with_entries(Vec::new());
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    evaluate_and_rollback(
        &store,
        &mut registry,
        &versions,
        &EvidenceBundle::default(),
        None,
    );
    assert_eq!(
        store.load().unwrap()[0].status,
        ObservationStatus::Observing
    );
}

#[test]
fn skill_effect_verified_is_distinct_from_load_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let observation = ChangeObservation::new_applied(
        "skill/issue-to-pr".into(),
        "cycle-new".into(),
        body_hash(""),
        body_hash("new"),
        vec!["skill_usage:issue-to-pr".into()],
        Vec::new(),
        vec!["run-before".into()],
    );
    store.append(&observation).expect("append");
    let mut registry = FakeRegistry::with_entries(Vec::new());
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    let evidence = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::SkillEffect,
            signal_id: "skill_effect:issue-to-pr:verified".into(),
            summary: "Skill issue-to-pr 使用后验证通过".into(),
            excerpt: None,
            occurrences: 1,
            sources: vec![crate::domain::evidence::EvidenceSourceRef {
                session_id: "project/session.dataset".into(),
                run_id: "run-after".into(),
                step_id: "step-1".into(),
            }],
        }],
    };
    evaluate_and_rollback(&store, &mut registry, &versions, &evidence, None);
    assert_eq!(store.load().unwrap()[0].status, ObservationStatus::Verified);
}

#[test]
fn repeated_ineffective_skill_rolls_back_to_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ChangeObservationStore::new(dir.path().join("changes.jsonl"));
    let mut observation = ChangeObservation::new_applied(
        "skill/issue-to-pr".into(),
        "cycle-new".into(),
        body_hash(""),
        body_hash("new"),
        vec!["skill_usage:issue-to-pr".into()],
        Vec::new(),
        vec!["run-before".into()],
    );
    observation.status = ObservationStatus::Ineffective;
    store.append(&observation).expect("append");
    let mut registry = FakeRegistry::with_entries(Vec::new());
    let versions = ComponentVersionStore::new(dir.path().join("versions"));
    let id = crate::domain::component::ComponentId {
        kind: crate::domain::component::ComponentKind::Skill,
        name: crate::domain::component::ComponentName::new("issue-to-pr"),
    };
    versions
        .capture(&registry, &id, "cycle-new", "upsert")
        .expect("snapshot");
    let evidence = EvidenceBundle {
        sessions_covered: 1,
        signals: vec![EvidenceSignal {
            kind: SignalKind::SkillEffect,
            signal_id: "skill_effect:issue-to-pr:ineffective".into(),
            summary: "Skill issue-to-pr 使用后验证失败".into(),
            excerpt: None,
            occurrences: 1,
            sources: vec![crate::domain::evidence::EvidenceSourceRef {
                session_id: "project/session.dataset".into(),
                run_id: "run-after".into(),
                step_id: "step-1".into(),
            }],
        }],
    };
    evaluate_and_rollback(&store, &mut registry, &versions, &evidence, None);
    assert_eq!(
        store.load().unwrap()[0].status,
        ObservationStatus::RolledBack
    );
}

#[test]
fn skill_effect_ignores_tests_before_load() {
    let trace = crate::domain::task_trace::TaskTrace {
        session_id: "project/session.dataset".into(),
        run_id: "run-1".into(),
        user_goal: "验证".into(),
        changed_files: Vec::new(),
        actions: vec![
            crate::domain::task_trace::TaskAction {
                kind: crate::domain::task_trace::TaskActionKind::Test,
                tool: "Bash".into(),
                command: Some("cargo test".into()),
                success: Some(false),
                target_path: None,
                excerpt: None,
            },
            crate::domain::task_trace::TaskAction {
                kind: crate::domain::task_trace::TaskActionKind::Other,
                tool: "Skill".into(),
                command: Some(r#"{"skill":"issue-to-pr"}"#.into()),
                success: None,
                target_path: None,
                excerpt: None,
            },
            crate::domain::task_trace::TaskAction {
                kind: crate::domain::task_trace::TaskActionKind::Test,
                tool: "Bash".into(),
                command: Some("cargo test".into()),
                success: Some(true),
                target_path: None,
                excerpt: None,
            },
        ],
        verification: Vec::new(),
        outcome: "Completed".into(),
        sources: Vec::new(),
    };
    let signals = crate::domain::task_trace::skill_effect_signals(&[trace]);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].signal_id, "skill_effect:issue-to-pr:verified");
}
