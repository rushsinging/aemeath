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
            sources: Vec::new(),
        }],
    };
    let summaries = evaluate_and_rollback(&store, &mut registry, &versions, &evidence);
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
            sources: Vec::new(),
        }],
    };
    evaluate_and_rollback(&store, &mut registry, &versions, &evidence);
    assert_eq!(
        registry.content_of(&id()).unwrap().body,
        "# Guardrails\n\n- external rule\n"
    );
    assert_eq!(store.load().unwrap()[0].status, ObservationStatus::Stale);
}
