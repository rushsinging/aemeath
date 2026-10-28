//! 即时查询分派测试。

use super::*;
use crate::application::loop_engine::chat::events::RuntimeStreamEvent;
use crate::ports::SessionQueryPort;
use config::{ConfigReader, ConfigRefreshOutcomeData};
use sdk::{ChatInputEvent, ModelSummary, ReflectionHistoryView, SdkError, SessionItemList};
use share::config::domain::snapshot::ConfigSnapshot;
use share::error::DomainError;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// 事件摘要：`RuntimeStreamEvent` 不实现 `Clone`，测试只保留断言所需字段。
#[derive(Debug, Clone)]
enum RecordedEvent {
    ModelList(Vec<ModelSummary>),
    ErrorText { text: String, is_error: bool },
    ConfigReloaded { changed_keys: Vec<String> },
    SystemMessage(String),
    Other,
}

#[derive(Clone, Default)]
struct RecordingSink(Arc<Mutex<Vec<RecordedEvent>>>);

impl RecordingSink {
    fn events(&self) -> Vec<RecordedEvent> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn record(&self, event: &RuntimeStreamEvent) {
        let recorded = match event {
            RuntimeStreamEvent::ModelList { models } => RecordedEvent::ModelList(models.clone()),
            RuntimeStreamEvent::CommandResultText { text, is_error } => RecordedEvent::ErrorText {
                text: text.clone(),
                is_error: *is_error,
            },
            RuntimeStreamEvent::ConfigReloaded { changed_keys, .. } => {
                RecordedEvent::ConfigReloaded {
                    changed_keys: changed_keys.clone(),
                }
            }
            RuntimeStreamEvent::SystemMessage(text) => RecordedEvent::SystemMessage(text.clone()),
            _ => RecordedEvent::Other,
        };
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(recorded);
    }
}

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(
        &'a self,
        event: RuntimeStreamEvent,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        self.record(&event);
        Box::pin(async {})
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.record(&event);
    }
}

/// 记录调用顺序（refresh 必须先于 list_models）。
#[derive(Clone, Default)]
struct CallOrder(Arc<Mutex<Vec<&'static str>>>);

impl CallOrder {
    fn push(&self, name: &'static str) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(name);
    }

    fn snapshot(&self) -> Vec<&'static str> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

struct FakeConfigReader {
    order: CallOrder,
    reloaded: bool,
}

#[async_trait::async_trait]
impl ConfigReader for FakeConfigReader {
    fn committed_snapshot(&self) -> ConfigSnapshot {
        ConfigSnapshot::new(Default::default())
    }

    fn subscribe_committed(&self) -> tokio::sync::watch::Receiver<ConfigSnapshot> {
        let (sender, receiver) =
            tokio::sync::watch::channel(ConfigSnapshot::new(Default::default()));
        let _ = sender;
        receiver
    }

    async fn refresh_if_sources_changed(&self) -> Result<ConfigRefreshOutcomeData, DomainError> {
        self.order.push("refresh");
        if self.reloaded {
            return Ok(ConfigRefreshOutcomeData::Reloaded {
                snapshot: ConfigSnapshot::new(Default::default()),
                scopes: vec![share::config::domain::scope::ConfigApplicationScope::Run],
            });
        }
        Ok(ConfigRefreshOutcomeData::Unchanged)
    }

    async fn snapshot(&self) -> Result<ConfigSnapshot, DomainError> {
        Ok(ConfigSnapshot::new(Default::default()))
    }

    async fn subscribe(&self) -> Result<config::ConfigSubscriptionData, DomainError> {
        Err(DomainError::unavailable("config", "配置读取暂不可用"))
    }
}

struct FakeSessionQueries {
    order: CallOrder,
    models: Mutex<Option<Result<Vec<ModelSummary>, SdkError>>>,
}

#[async_trait::async_trait]
impl SessionQueryPort for FakeSessionQueries {
    async fn list_models(&self) -> Result<Vec<ModelSummary>, SdkError> {
        self.order.push("list_models");
        self.models
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .unwrap_or(Ok(Vec::new()))
    }

    async fn list_sessions(&self) -> Result<Vec<SessionItemList>, SdkError> {
        Ok(Vec::new())
    }

    async fn list_reflection_history(
        &self,
        _limit: usize,
    ) -> Result<Vec<ReflectionHistoryView>, SdkError> {
        Ok(Vec::new())
    }
}

fn dispatcher(
    order: &CallOrder,
    models: Result<Vec<ModelSummary>, SdkError>,
) -> ImmediateQueryDispatcher {
    ImmediateQueryDispatcher::new(
        Arc::new(FakeSessionQueries {
            order: order.clone(),
            models: Mutex::new(Some(models)),
        }),
        Arc::new(FakeConfigReader {
            order: order.clone(),
            reloaded: false,
        }),
    )
}

fn sample_model() -> ModelSummary {
    ModelSummary {
        provider: "Aliyun Token Plan".to_string(),
        id: "qwen3.8-max".to_string(),
        name: "Qwen3.8 Max".to_string(),
        context_window: 1_000_000,
        max_tokens: 20_000,
    }
}

#[tokio::test]
async fn non_query_event_is_not_consumed() {
    let order = CallOrder::default();
    let sink = RecordingSink::default();
    let dispatcher = dispatcher(&order, Ok(vec![sample_model()]));

    let consumed = dispatcher
        .try_dispatch(&ChatInputEvent::QueryReflectionHistory { limit: 5 }, &sink)
        .await;

    assert!(!consumed, "查询历史不是即时查询，必须继续走排队路径");
    assert!(sink.events().is_empty(), "未消费事件不得发出任何事件");
    assert!(order.snapshot().is_empty(), "未消费事件不得触发配置刷新");
}

#[tokio::test]
async fn list_models_is_consumed_and_emits_model_list() {
    let order = CallOrder::default();
    let sink = RecordingSink::default();
    let dispatcher = dispatcher(&order, Ok(vec![sample_model()]));

    let consumed = dispatcher
        .try_dispatch(&ChatInputEvent::ListModels, &sink)
        .await;

    assert!(consumed, "ListModels 必须被即时分派消费");
    match sink.events().as_slice() {
        [RecordedEvent::ModelList(models)] => {
            assert_eq!(models.len(), 1);
            assert_eq!(models[0].id, "qwen3.8-max");
        }
        other => panic!("期望单个 ModelList 事件，实际 {other:?}"),
    }
}

#[tokio::test]
async fn list_models_refreshes_config_before_querying() {
    let order = CallOrder::default();
    let sink = RecordingSink::default();
    let dispatcher = dispatcher(&order, Ok(Vec::new()));

    dispatcher
        .try_dispatch(&ChatInputEvent::ListModels, &sink)
        .await;

    assert_eq!(
        order.snapshot(),
        vec!["refresh", "list_models"],
        "模型清单必须先刷新配置源指纹，再读取快照"
    );
}

#[tokio::test]
async fn list_models_failure_emits_error_text() {
    let order = CallOrder::default();
    let sink = RecordingSink::default();
    let dispatcher = dispatcher(
        &order,
        Err(SdkError::Internal("config query unavailable".to_string())),
    );

    let consumed = dispatcher
        .try_dispatch(&ChatInputEvent::ListModels, &sink)
        .await;

    assert!(consumed);
    match sink.events().as_slice() {
        [RecordedEvent::ErrorText { text, is_error }] => {
            assert!(is_error, "查询失败必须以错误文本回传");
            assert!(text.contains("List models failed"), "实际: {text}");
        }
        other => panic!("期望错误文本事件，实际 {other:?}"),
    }
}

#[tokio::test]
async fn each_query_rechecks_config_freshness() {
    let order = CallOrder::default();
    let sink = RecordingSink::default();
    let dispatcher = dispatcher(&order, Ok(Vec::new()));

    for _ in 0..2 {
        dispatcher
            .try_dispatch(&ChatInputEvent::ListModels, &sink)
            .await;
    }

    assert_eq!(
        order.snapshot(),
        vec!["refresh", "list_models", "refresh", "list_models"],
        "每次查询都必须重新校验配置新鲜度"
    );
}

fn dispatcher_with_reload(
    order: &CallOrder,
    models: Result<Vec<ModelSummary>, SdkError>,
) -> ImmediateQueryDispatcher {
    ImmediateQueryDispatcher::new(
        Arc::new(FakeSessionQueries {
            order: order.clone(),
            models: Mutex::new(Some(models)),
        }),
        Arc::new(FakeConfigReader {
            order: order.clone(),
            reloaded: true,
        }),
    )
}

/// `/model` 前的刷新若真正提交了新配置，MUST 在返回模型清单之前先发
/// ConfigReloaded + 可见 SystemMessage；否则该路径会静默吞掉变更，
/// 之后的 turn 边界只看到 Unchanged，用户永远看不到「已重载」。
#[tokio::test]
async fn list_models_reports_config_reload_before_returning_models() {
    let order = CallOrder::default();
    let sink = RecordingSink::default();
    let dispatcher = dispatcher_with_reload(&order, Ok(vec![sample_model()]));

    let consumed = dispatcher
        .try_dispatch(&ChatInputEvent::ListModels, &sink)
        .await;
    assert!(consumed, "ListModels 属于即时查询，必须被就地消费");

    let events = sink.events();
    assert!(
        matches!(
            events.first(),
            Some(RecordedEvent::ConfigReloaded { changed_keys })
                if changed_keys.contains(&"config:reloaded".to_string())
                    && changed_keys.contains(&"config:scope:run".to_string())
        ),
        "首个事件必须是带 scope key 的 ConfigReloaded，实际: {events:?}"
    );
    assert!(
        matches!(
            events.get(1),
            Some(RecordedEvent::SystemMessage(text))
                if text.contains("[config] 已重载") || text.contains("[config] reloaded")
        ),
        "第二个事件必须是可见回显 SystemMessage，实际: {events:?}"
    );
    assert!(
        matches!(events.last(), Some(RecordedEvent::ModelList(models)) if models.len() == 1),
        "模型清单必须在回显之后返回，实际: {events:?}"
    );
    assert_eq!(
        order.snapshot(),
        vec!["refresh", "list_models"],
        "刷新必须先于清单读取"
    );
}
