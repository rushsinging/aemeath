//! `CompactModelResolver` 的模型选择、窗口与缓存语义。

use super::*;
use crate::ports::provider_port::fake::FakeProvider;
use crate::ports::ProviderBuildSpec;
use provider::ModelId;
use provider::ReasoningLevel;
use share::config::models::{ModelEntryConfig, ProviderModelsConfig};
use share::config::Config;
use std::sync::atomic::{AtomicUsize, Ordering};

struct FakeConfigReader {
    snapshot: std::sync::Mutex<ConfigSnapshot>,
    changes: tokio::sync::watch::Sender<ConfigSnapshot>,
}

impl FakeConfigReader {
    fn new(snapshot: ConfigSnapshot) -> Self {
        let (changes, _receiver) = tokio::sync::watch::channel(snapshot.clone());
        Self {
            snapshot: std::sync::Mutex::new(snapshot),
            changes,
        }
    }

    fn set_snapshot(&self, snapshot: ConfigSnapshot) {
        *self
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = snapshot.clone();
        let _ = self.changes.send(snapshot);
    }
}

#[async_trait::async_trait]
impl config::ConfigReader for FakeConfigReader {
    fn committed_snapshot(&self) -> ConfigSnapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn subscribe_committed(&self) -> tokio::sync::watch::Receiver<ConfigSnapshot> {
        self.changes.subscribe()
    }

    async fn refresh_if_sources_changed(&self) -> config::ConfigRefreshOutcome {
        config::ConfigRefreshOutcome::Unchanged
    }
}

#[derive(Default)]
struct RecordingFactory {
    builds: Arc<AtomicUsize>,
}

impl ProviderFactory for RecordingFactory {
    fn build(&self, spec: ProviderBuildSpec) -> Result<ProviderBinding, provider::ProviderError> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        Ok(ProviderBinding {
            provider: Arc::new(FakeProvider::new()),
            model: spec.model,
            max_tokens: spec.max_tokens,
            requested_reasoning: spec.requested_reasoning,
            context_window: spec.context_window,
        })
    }
}

fn snapshot_with(compact_model: Option<&str>) -> ConfigSnapshot {
    let mut config = Config::default();
    config.models.default = "local/session-model".into();
    config.models.providers.insert(
        "local".into(),
        ProviderModelsConfig {
            driver: "openai".into(),
            api_key: "test-key".into(),
            models: vec![
                ModelEntryConfig {
                    id: "session-model".into(),
                    name: "Session Model".into(),
                    context_window: 200_000,
                    max_tokens: 8_192,
                    ..Default::default()
                },
                ModelEntryConfig {
                    id: "compact-model".into(),
                    name: "Compact Model".into(),
                    context_window: 32_000,
                    max_tokens: 4_096,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    );
    if let Some(selection) = compact_model {
        config.context.compact_model = Some(selection.to_string());
    }
    ConfigSnapshot::new(config)
}

fn resolver(
    snapshot: ConfigSnapshot,
    session_model: SessionModelSlot,
) -> (
    CompactModelResolver,
    Arc<AtomicUsize>,
    Arc<FakeConfigReader>,
) {
    let builds = Arc::new(AtomicUsize::new(0));
    let reader = Arc::new(FakeConfigReader::new(snapshot));
    let resolver = CompactModelResolver::new(
        reader.clone(),
        Arc::new(RecordingFactory {
            builds: builds.clone(),
        }),
        session_model,
    );
    (resolver, builds, reader)
}

fn session_state(
    snapshot: &ConfigSnapshot,
    selection: &str,
    context_window: usize,
) -> crate::application::client::SessionModelState {
    let resolved = snapshot
        .resolve_model_selection(selection)
        .expect("session model must resolve");
    let model_id = resolved.model.id.clone();
    crate::application::client::SessionModelState::new(
        resolved,
        Arc::new(ProviderBinding {
            provider: Arc::new(FakeProvider::new()),
            model: ModelId {
                provider: "local".into(),
                model: model_id,
            },
            max_tokens: 8_192,
            requested_reasoning: ReasoningLevel::Off,
            context_window: Some(context_window),
        }),
    )
}

fn bound_slot(snapshot: &ConfigSnapshot) -> SessionModelSlot {
    let slot = SessionModelSlot::new();
    slot.bind(session_state(snapshot, "local/session-model", 200_000));
    slot
}

#[test]
fn unset_compact_model_follows_current_session_model() {
    let snapshot = snapshot_with(None);
    let (resolver, builds, _reader) = resolver(snapshot.clone(), bound_slot(&snapshot));

    let target = resolver.resolve().expect("session model must resolve");

    assert_eq!(target.origin(), CompactModelOrigin::SessionModel);
    assert_eq!(target.binding().model.model, "session-model");
    assert_eq!(target.context_window(), Some(200_000));
    assert_eq!(
        builds.load(Ordering::SeqCst),
        0,
        "跟随会话模型不应构建新 binding"
    );
}

#[test]
fn configured_compact_model_uses_configured_selection() {
    let snapshot = snapshot_with(Some("local/compact-model"));
    let (resolver, builds, _reader) = resolver(snapshot.clone(), bound_slot(&snapshot));

    let target = resolver.resolve().expect("configured model must resolve");

    assert_eq!(target.origin(), CompactModelOrigin::Configured);
    assert_eq!(target.binding().model.model, "compact-model");
    assert_eq!(target.context_window(), Some(32_000));
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert!(target.model_identity().contains("compact-model"));
}

#[test]
fn configured_selection_reuses_cached_binding_across_resolves() {
    let snapshot = snapshot_with(Some("local/compact-model"));
    let (resolver, builds, _reader) = resolver(snapshot.clone(), bound_slot(&snapshot));

    let first = resolver.resolve().expect("first resolve");
    let second = resolver.resolve().expect("second resolve");

    assert_eq!(first.binding().model.model, second.binding().model.model);
    assert_eq!(
        builds.load(Ordering::SeqCst),
        1,
        "同一 selection 重复 compact 不得重复构建 binding"
    );
}

#[test]
fn changed_selection_rebuilds_binding() {
    let first_snapshot = snapshot_with(Some("local/compact-model"));
    let (resolver, builds, reader) = resolver(first_snapshot.clone(), bound_slot(&first_snapshot));
    assert_eq!(
        resolver
            .resolve()
            .expect("first resolve")
            .binding()
            .model
            .model,
        "compact-model"
    );

    reader.set_snapshot(snapshot_with(Some("local/session-model")));

    assert_eq!(
        resolver
            .resolve()
            .expect("second resolve")
            .binding()
            .model
            .model,
        "session-model"
    );
    assert_eq!(
        builds.load(Ordering::SeqCst),
        2,
        "selection 变化必须重建 binding"
    );
}

#[test]
fn unknown_configured_selection_returns_typed_error() {
    let snapshot = snapshot_with(Some("local/missing-model"));
    let (resolver, _builds, _reader) = resolver(snapshot.clone(), bound_slot(&snapshot));

    let error = resolver
        .resolve()
        .expect_err("未知 selection 必须报错，而不是回退会话模型");

    assert!(
        matches!(error, CompactModelResolveError::Selection(_)),
        "实际错误：{error:?}"
    );
}

#[test]
fn unbound_session_model_without_configuration_returns_typed_error() {
    let snapshot = snapshot_with(None);
    let (resolver, _builds, _reader) = resolver(snapshot, SessionModelSlot::new());

    let error = resolver.resolve().expect_err("未绑定会话模型时必须报错");

    assert_eq!(error, CompactModelResolveError::SessionModelUnavailable);
}

#[test]
fn session_model_switch_is_observed_by_later_resolves() {
    let snapshot = snapshot_with(None);
    let slot = SessionModelSlot::new();
    slot.bind(session_state(&snapshot, "local/session-model", 200_000));
    let (resolver, _builds, _reader) = resolver(snapshot.clone(), slot.clone());

    assert_eq!(
        resolver
            .resolve()
            .expect("before switch")
            .binding()
            .model
            .model,
        "session-model"
    );

    slot.bind(session_state(&snapshot, "local/compact-model", 32_000));

    let target = resolver.resolve().expect("after switch");
    assert_eq!(target.binding().model.model, "compact-model");
    assert_eq!(target.context_window(), Some(32_000));
}
