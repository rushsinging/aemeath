use super::*;

fn candidate() -> RsiCandidate {
    RsiCandidate::new_pending(
        CandidateScope::Global,
        "tool-guidance/default".to_string(),
        "old".to_string(),
        "new".to_string(),
        vec!["retry_pattern:Bash".to_string()],
        vec!["project-a".to_string()],
        CandidateRisk::Medium,
    )
}

#[test]
fn store_round_trips_and_updates_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = RsiCandidateStore::new(dir.path().join("candidates.jsonl"));
    let candidate = candidate();
    store.append(&candidate).expect("append");
    let loaded = store.load().expect("load");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, candidate.id);
    assert_eq!(store.pending().expect("pending").len(), 1);

    let updated = store
        .update(
            candidate.id.as_str(),
            CandidateStatus::Rejected,
            Some("test".to_string()),
        )
        .expect("update");
    assert_eq!(updated.status, CandidateStatus::Rejected);
    assert!(store.pending().expect("pending").is_empty());
}

#[test]
fn find_accepts_typed_and_unknown_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = RsiCandidateStore::new(dir.path().join("candidates.jsonl"));
    let candidate = candidate();
    store.append(&candidate).expect("append");
    assert!(store.find(candidate.id.as_str()).expect("find").is_some());
    assert!(store.find("rsc_missing").expect("find").is_none());
}

#[test]
fn manual_typed_candidate_json_parses() {
    let json = r#"{"id":"rsc_00000000001","scope":"global","target_component":"guardrails/default","base_body":"a","proposed_body":"b","evidence":[],"projects":[],"risk":"medium","conflict":null,"status":"pending","created_at_secs":1,"reviewed_at_secs":null,"review_reason":null}"#;
    assert!(RsiCandidateId::parse("rsc_00000000001").is_ok());
    let candidate: RsiCandidate = serde_json::from_str(json).expect("parse candidate");
    assert_eq!(candidate.id.as_str(), "rsc_00000000001");
}
