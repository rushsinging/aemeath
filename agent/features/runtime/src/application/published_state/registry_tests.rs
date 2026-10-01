use super::*;

fn decision() -> context::CompactionDecisionData {
    context::CompactionDecisionData {
        needed: true,
        urgency: context::Urgency::Should,
        decision_token_count: 145_000,
        threshold: 144_000,
        context_size: 200_000,
        effective_window: 180_000,
        reason: context::DecisionReason::ActualProviderUsage,
    }
}

#[test]
fn heartbeat_keeps_business_revision() {
    let registry = PublishedStateRegistry::default();
    let status = registry.update_context_budget("session-1", &decision());
    let heartbeat = registry.heartbeat().expect("status");
    assert_eq!(status.revision, heartbeat.revision);
    assert_eq!(heartbeat.heartbeat_sequence, 1);
}

#[test]
fn session_reset_starts_a_new_revision_epoch() {
    let registry = PublishedStateRegistry::default();
    registry.update_context_budget("session-1", &decision());
    registry.reset_session("session-2");
    let status = registry.update_context_budget("session-2", &decision());
    assert_eq!(status.session_id, "session-2");
    assert_eq!(status.revision, 1);
    assert_eq!(status.heartbeat_sequence, 0);
}
