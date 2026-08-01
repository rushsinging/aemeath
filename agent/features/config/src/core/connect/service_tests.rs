use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::commit::test_helpers::{CommitOutcome, StubCommitPort};
use super::*;
use crate::catalog::{find_by_source, PROVIDER_CATALOG};

struct StubProbe {
    outcome: tokio::sync::Mutex<Result<ProviderProbeResult, ProviderProbeError>>,
}

impl StubProbe {
    fn success() -> Arc<Self> {
        Arc::new(Self {
            outcome: tokio::sync::Mutex::new(Ok(ProviderProbeResult {
                latency: Duration::from_millis(3),
            })),
        })
    }

    fn failure(kind: ProviderProbeErrorKind) -> Arc<Self> {
        Arc::new(Self {
            outcome: tokio::sync::Mutex::new(Err(ProviderProbeError {
                kind,
                message: "测试探测失败".to_string(),
            })),
        })
    }
}

#[async_trait]
impl ProviderProbePort for StubProbe {
    async fn probe(
        &self,
        _request: ProviderProbeRequest,
    ) -> Result<ProviderProbeResult, ProviderProbeError> {
        self.outcome.lock().await.clone()
    }
}

struct BlockingProbe {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl BlockingProbe {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        })
    }
}

#[async_trait]
impl ProviderProbePort for BlockingProbe {
    async fn probe(
        &self,
        _request: ProviderProbeRequest,
    ) -> Result<ProviderProbeResult, ProviderProbeError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(ProviderProbeResult {
            latency: Duration::from_millis(1),
        })
    }
}

struct BlockingCommit {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl BlockingCommit {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        })
    }
}

#[async_trait]
impl ConnectCommitPort for BlockingCommit {
    async fn commit(
        &self,
        _request: ConnectCommitRequest,
    ) -> Result<ConnectCommitReceipt, ConnectCommitError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(ConnectCommitReceipt {
            applied_revision: 11,
        })
    }
}

fn test_global_revision() -> crate::GlobalConfigRevision {
    crate::GlobalConfigRevision::from_digest("test-global-revision")
}

async fn advance(
    service: &ConnectAppService,
    view: ConnectView,
    command: ConnectCommand,
) -> ConnectView {
    service
        .apply(view.session_id, view.revision, command)
        .await
        .expect("command should succeed")
}

async fn ready_to_probe(service: &ConnectAppService) -> ConnectView {
    let mut view = service
        .start_connect(ConnectOrigin::ExplicitCommand, test_global_revision(), None)
        .await;
    view = advance(
        service,
        view,
        ConnectCommand::SelectProvider {
            source: find_by_source("Anthropic").unwrap().source,
        },
    )
    .await;
    view = advance(
        service,
        view,
        ConnectCommand::SetEndpoint {
            base_url: "https://example.test".into(),
        },
    )
    .await;
    view = advance(
        service,
        view,
        ConnectCommand::SetCredential {
            api_key: "secret-key".into(),
        },
    )
    .await;
    view = advance(
        service,
        view,
        ConnectCommand::SetProviderUserAgent { raw: None },
    )
    .await;
    view = advance(service, view, ConnectCommand::EnterCustomModel).await;
    view = advance(
        service,
        view,
        ConnectCommand::SetCustomModel {
            model_id: "model-1".into(),
            context_window: 32_000,
            max_tokens: 4_096,
        },
    )
    .await;
    advance(
        service,
        view,
        ConnectCommand::SetGlobalDefault {
            set_as_default: true,
        },
    )
    .await
}

async fn ready_to_review(service: &ConnectAppService) -> ConnectView {
    let view = ready_to_probe(service).await;
    advance(service, view, ConnectCommand::SkipProbe).await
}

#[test]
fn session_identity_and_revision_round_trip_through_transport_values() {
    let session_id = ConnectSessionId::new();
    let encoded = session_id.to_transport_string();
    assert_eq!(
        ConnectSessionId::from_transport_str(&encoded).unwrap(),
        session_id
    );

    let revision = ConnectRevision::initial().bump();
    assert_eq!(ConnectRevision::from_value(revision.value()), revision);
}

#[tokio::test]
async fn selecting_verified_provider_prefills_catalog_endpoint_and_recommended_model() {
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::success())
        .build();
    let view = service
        .start_connect(ConnectOrigin::ExplicitCommand, test_global_revision(), None)
        .await;

    let endpoint = advance(
        &service,
        view,
        ConnectCommand::SelectProvider {
            source: find_by_source("Anthropic").unwrap().source,
        },
    )
    .await;
    assert_eq!(endpoint.stage, ConnectStage::EditEndpoint);
    assert_eq!(endpoint.draft.base_url(), Some("https://api.anthropic.com"));

    let credential = advance(
        &service,
        endpoint,
        ConnectCommand::SetEndpoint {
            base_url: "https://api.anthropic.com".into(),
        },
    )
    .await;
    let user_agent = advance(
        &service,
        credential,
        ConnectCommand::SetCredential {
            api_key: String::new(),
        },
    )
    .await;
    let models = advance(
        &service,
        user_agent,
        ConnectCommand::SetProviderUserAgent { raw: None },
    )
    .await;
    let selected = advance(
        &service,
        models,
        ConnectCommand::SelectRecommendedModel { index: 0 },
    )
    .await;
    assert_eq!(selected.stage, ConnectStage::ChooseGlobalDefault);
    assert_eq!(
        selected
            .draft
            .model
            .as_ref()
            .map(|model| model.model_id.as_str()),
        Some("claude-sonnet-5")
    );
}

#[tokio::test]
async fn custom_model_skip_probe_save_completes_without_exposing_api_key() {
    let commit = StubCommitPort::new(CommitOutcome::Success {
        applied_revision: 7,
    });
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::success())
        .with_commit(commit.clone())
        .build();

    let view = ready_to_review(&service).await;
    assert_eq!(view.stage, ConnectStage::Review);
    assert!(view.draft.has_api_key);
    assert!(!format!("{view:?}").contains("secret-key"));

    let completed = advance(&service, view, ConnectCommand::ConfirmSave).await;
    assert_eq!(completed.stage, ConnectStage::Completed);
    assert_eq!(
        completed.terminal,
        Some(ConnectOutcome::Completed {
            applied_revision: 7
        })
    );
    assert_eq!(commit.requests.lock().await.len(), 1);
    assert_eq!(
        commit.requests.lock().await[0]
            .expected_global_revision
            .as_str(),
        test_global_revision().as_str()
    );
}

#[tokio::test]
async fn back_from_each_edit_stage_returns_to_previous_stage_and_preserves_draft() {
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::success())
        .build();
    let initial = service
        .start_connect(ConnectOrigin::ExplicitCommand, test_global_revision(), None)
        .await;
    let endpoint = advance(
        &service,
        initial,
        ConnectCommand::SelectProvider {
            source: find_by_source("Anthropic").unwrap().source,
        },
    )
    .await;
    let credential = advance(
        &service,
        endpoint.clone(),
        ConnectCommand::SetEndpoint {
            base_url: "https://custom.example.test".into(),
        },
    )
    .await;

    let returned = advance(&service, credential, ConnectCommand::Back).await;

    assert_eq!(returned.stage, ConnectStage::EditEndpoint);
    assert_eq!(
        returned.draft.base_url(),
        Some("https://custom.example.test")
    );
    let provider_selection = advance(&service, returned, ConnectCommand::Back).await;
    assert_eq!(provider_selection.stage, ConnectStage::SelectProvider);
    assert_eq!(
        provider_selection
            .draft
            .source
            .map(|source| source.as_str()),
        Some("Anthropic")
    );
}

#[tokio::test]
async fn back_from_initial_provider_stage_is_rejected_without_cancelling_session() {
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::success())
        .build();
    let initial = service
        .start_connect(ConnectOrigin::ExplicitCommand, test_global_revision(), None)
        .await;

    let error = service
        .apply(initial.session_id, initial.revision, ConnectCommand::Back)
        .await
        .expect_err("initial stage has no previous page");

    assert!(matches!(error, ConnectError::InvalidTransition { .. }));
    let current = service.view(initial.session_id).await.unwrap();
    assert_eq!(current.stage, ConnectStage::SelectProvider);
    assert_eq!(current.terminal, None);
}

#[tokio::test]
async fn stale_revision_rejects_command_without_changing_view() {
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::success())
        .build();
    let view = service
        .start_connect(ConnectOrigin::ExplicitCommand, test_global_revision(), None)
        .await;
    let error = service
        .apply(
            view.session_id,
            ConnectRevision::from_value(99),
            ConnectCommand::SelectProvider {
                source: find_by_source("Anthropic").unwrap().source,
            },
        )
        .await
        .expect_err("stale revision must fail");
    assert!(matches!(error, ConnectError::StaleRevision { .. }));
    let current = service.view(view.session_id).await.unwrap();
    assert_eq!(current.stage, ConnectStage::SelectProvider);
    assert_eq!(current.revision, view.revision);
}

#[tokio::test]
async fn cancelled_session_rejects_repeated_terminal_command() {
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::success())
        .build();
    let view = service
        .start_connect(ConnectOrigin::ExplicitCommand, test_global_revision(), None)
        .await;
    let cancelled = service
        .cancel(view.session_id, view.revision)
        .await
        .unwrap();
    assert_eq!(cancelled.terminal, Some(ConnectOutcome::Cancelled));
    let error = service
        .cancel(cancelled.session_id, cancelled.revision)
        .await
        .expect_err("terminal command must not repeat");
    assert!(matches!(error, ConnectError::InvalidTransition { .. }));
}

#[tokio::test]
async fn probe_success_moves_directly_to_review() {
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::success())
        .build();
    let view = ready_to_probe(&service).await;
    let result = advance(&service, view, ConnectCommand::BeginProbe).await;
    assert_eq!(result.stage, ConnectStage::Review);
    assert!(matches!(
        result.probe_status,
        Some(ProbeStatusView::Success { .. })
    ));
}

#[tokio::test]
async fn probe_failure_requires_explicit_continue_or_edit() {
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::failure(ProviderProbeErrorKind::Timeout))
        .build();
    let view = ready_to_probe(&service).await;
    let failed = advance(&service, view, ConnectCommand::BeginProbe).await;
    assert_eq!(failed.stage, ConnectStage::Probing);
    assert!(matches!(
        failed.probe_status,
        Some(ProbeStatusView::Failed {
            kind: ProviderProbeErrorKind::Timeout,
            ..
        })
    ));
    let review = advance(&service, failed.clone(), ConnectCommand::ContinueAfterProbe).await;
    assert_eq!(review.stage, ConnectStage::Review);
}

#[tokio::test]
async fn rejecting_existing_provider_returns_to_selection() {
    let existing = ExistingProviderSnapshot::from_provider_config(
        "Anthropic",
        "https://existing.test",
        Some("hidden-key"),
        Some("anthropic"),
        "existing-model",
        16_000,
        2_000,
    );
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::success())
        .build();
    let view = service
        .start_connect(
            ConnectOrigin::ExplicitCommand,
            test_global_revision(),
            Some(existing),
        )
        .await;
    assert_eq!(view.stage, ConnectStage::ConfirmOverwrite);
    assert!(!format!("{view:?}").contains("hidden-key"));
    let rejected = advance(&service, view, ConnectCommand::RejectOverwrite).await;
    assert_eq!(rejected.stage, ConnectStage::SelectProvider);
    assert!(rejected.draft.source.is_none());
}

#[tokio::test]
async fn commit_conflict_remains_saving_and_can_retry() {
    let commit = StubCommitPort::new(CommitOutcome::Failure(
        ConnectCommitError::PersistConflict { expected: 2 },
    ));
    let service = ConnectAppService::builder()
        .with_catalog(PROVIDER_CATALOG)
        .with_probe(StubProbe::success())
        .with_commit(commit.clone())
        .build();
    let view = ready_to_review(&service).await;
    let conflicted = advance(&service, view, ConnectCommand::ConfirmSave).await;
    assert_eq!(conflicted.stage, ConnectStage::Saving);
    assert!(matches!(
        conflicted.last_error,
        Some(ConnectError::PersistConflict { expected: 2 })
    ));
    commit
        .set_outcome(CommitOutcome::Success {
            applied_revision: 3,
        })
        .await;
    let completed = advance(&service, conflicted, ConnectCommand::ConfirmSave).await;
    assert_eq!(completed.stage, ConnectStage::Completed);
}

#[tokio::test]
async fn cancel_same_session_during_probe_wins_over_late_probe_result() {
    let probe = BlockingProbe::new();
    let service = Arc::new(
        ConnectAppService::builder()
            .with_catalog(PROVIDER_CATALOG)
            .with_probe(probe.clone())
            .build(),
    );
    let probing_view = ready_to_probe(&service).await;
    let service_for_probe = service.clone();
    let probe_task = tokio::spawn(async move {
        service_for_probe
            .apply(
                probing_view.session_id,
                probing_view.revision,
                ConnectCommand::BeginProbe,
            )
            .await
    });
    probe.entered.notified().await;

    let running = tokio::time::timeout(
        Duration::from_millis(100),
        service.view(probing_view.session_id),
    )
    .await
    .expect("same session view must remain available during probe")
    .expect("session exists");
    assert_eq!(running.stage, ConnectStage::Probing);
    let cancelled = tokio::time::timeout(
        Duration::from_millis(100),
        service.cancel(running.session_id, running.revision),
    )
    .await
    .expect("cancel must not wait for probe")
    .expect("cancel succeeds");
    assert_eq!(cancelled.terminal, Some(ConnectOutcome::Cancelled));

    probe.release.notify_one();
    let late = probe_task.await.unwrap().unwrap();
    assert_eq!(late.stage, ConnectStage::Cancelled);
    assert_eq!(late.terminal, Some(ConnectOutcome::Cancelled));
    assert!(!matches!(
        late.terminal,
        Some(ConnectOutcome::Completed { .. })
    ));
}

#[tokio::test]
async fn cancel_same_session_during_commit_wins_over_late_success() {
    let commit = BlockingCommit::new();
    let service = Arc::new(
        ConnectAppService::builder()
            .with_catalog(PROVIDER_CATALOG)
            .with_probe(StubProbe::success())
            .with_commit(commit.clone())
            .build(),
    );
    let review = ready_to_review(&service).await;
    let service_for_commit = service.clone();
    let commit_task = tokio::spawn(async move {
        service_for_commit
            .apply(
                review.session_id,
                review.revision,
                ConnectCommand::ConfirmSave,
            )
            .await
    });
    commit.entered.notified().await;

    let saving = tokio::time::timeout(Duration::from_millis(100), service.view(review.session_id))
        .await
        .expect("same session view must remain available during commit")
        .expect("session exists");
    assert_eq!(saving.stage, ConnectStage::Saving);
    let cancelled = tokio::time::timeout(
        Duration::from_millis(100),
        service.cancel(saving.session_id, saving.revision),
    )
    .await
    .expect("cancel must not wait for commit")
    .expect("cancel succeeds");
    assert_eq!(cancelled.terminal, Some(ConnectOutcome::Cancelled));

    commit.release.notify_one();
    let late = commit_task.await.unwrap().unwrap();
    assert_eq!(late.stage, ConnectStage::Cancelled);
    assert_eq!(late.terminal, Some(ConnectOutcome::Cancelled));
    assert!(!matches!(
        late.terminal,
        Some(ConnectOutcome::Completed { .. })
    ));
}

#[tokio::test]
async fn unrelated_session_view_is_not_blocked_while_probe_waits() {
    let probe = BlockingProbe::new();
    let service = Arc::new(
        ConnectAppService::builder()
            .with_catalog(PROVIDER_CATALOG)
            .with_probe(probe.clone())
            .build(),
    );
    let probing_view = ready_to_probe(&service).await;
    let other = service
        .start_connect(ConnectOrigin::ExplicitCommand, test_global_revision(), None)
        .await;

    let service_for_probe = service.clone();
    let probe_task = tokio::spawn(async move {
        service_for_probe
            .apply(
                probing_view.session_id,
                probing_view.revision,
                ConnectCommand::BeginProbe,
            )
            .await
    });
    probe.entered.notified().await;

    let other_view =
        tokio::time::timeout(Duration::from_millis(100), service.view(other.session_id))
            .await
            .expect("another session must not wait for probe")
            .expect("other session exists");
    assert_eq!(other_view.stage, ConnectStage::SelectProvider);

    probe.release.notify_one();
    probe_task.await.unwrap().unwrap();
}
