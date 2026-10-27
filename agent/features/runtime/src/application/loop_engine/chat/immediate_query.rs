//! 即时查询分派。
//!
//! 查询类输入事件（当前为 `ListModels`）语义是 request/response，不改变会话
//! 状态，因此 NEVER 进入 gate / pending command：在输入接收点就地执行并回事件。
//!
//! # ingress 契约（#2042）
//!
//! 所有输入接收路径 MUST 在 admit / 入队之前先过 [`ImmediateQueryDispatcher::try_dispatch`]
//! （经 `BufferedInputAdapter::dispatch_immediate_query` 或直接引用）：
//! - turn 边界 drain 与 AwaitUser park：`input_strategy.rs`（adapter 统一持有）；
//! - busy 模型回调：`main_run_port.rs`（经 adapter 统一入口）；
//! - idle 接收：`idle_lifecycle.rs`（独立引用同一实现）。
//!
//! 新增接收路径漏接会表现为查询事件被 TUI 回显为排队命令（如 `/model`）且
//! 刷新在 idle gate 兜底 drop 中静默丢失。
//!
//! 查询前先比对配置源指纹，保证模型清单反映磁盘最新配置；否则运行中修改
//! `aemeath.json` 后要等到下一个 turn 边界才可见。

use crate::application::loop_engine::chat::events::{ChatEventSink, RuntimeStreamEvent};
use crate::ports::SessionQueryPort;
use config::ConfigReader;
use sdk::ChatInputEvent;
use std::sync::Arc;

/// 即时查询分派器：持有查询所需的窄端口，供 run 与 idle 两条接收路径共用。
#[derive(Clone)]
pub(crate) struct ImmediateQueryDispatcher {
    session_queries: Arc<dyn SessionQueryPort>,
    config_reader: Arc<dyn ConfigReader>,
}

impl ImmediateQueryDispatcher {
    pub(crate) fn new(
        session_queries: Arc<dyn SessionQueryPort>,
        config_reader: Arc<dyn ConfigReader>,
    ) -> Self {
        Self {
            session_queries,
            config_reader,
        }
    }

    /// 就地处理查询类事件。
    ///
    /// 返回 `true` 表示事件已被消费，调用方 **NEVER** 再将其入队或转成
    /// `PendingCommand`；返回 `false` 表示不是查询事件，按既有路径排队。
    pub(crate) async fn try_dispatch<S>(&self, event: &ChatInputEvent, sink: &S) -> bool
    where
        S: ChatEventSink,
    {
        if !matches!(event, ChatInputEvent::ListModels) {
            return false;
        }
        self.dispatch_list_models(sink).await;
        true
    }

    async fn dispatch_list_models<S>(&self, sink: &S)
    where
        S: ChatEventSink,
    {
        if let Err(error) = self.config_reader.refresh_if_sources_changed().await {
            log::warn!(
                target: crate::LOG_TARGET,
                "[immediate_query] config refresh failed: {error}"
            );
        }
        match self.session_queries.list_models().await {
            Ok(models) => {
                sink.send_event(RuntimeStreamEvent::ModelList { models })
                    .await;
            }
            Err(error) => {
                sink.send_event(RuntimeStreamEvent::CommandResultText {
                    text: format!("List models failed: {error}"),
                    is_error: true,
                })
                .await;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! 供其它 runtime 测试复用的无 IO 分派器：查询端口返回空列表，配置刷新恒为 Unchanged。

    use super::*;
    use config::ConfigRefreshOutcomeData;
    use sdk::{ModelSummary, ReflectionHistoryView, SdkError, SessionItemList};
    use share::config::domain::snapshot::ConfigSnapshot;
    use share::error::DomainError;

    pub(crate) fn no_io_dispatcher() -> ImmediateQueryDispatcher {
        ImmediateQueryDispatcher::new(Arc::new(NoopSessionQueries), Arc::new(NoopConfigReader))
    }

    struct NoopSessionQueries;

    #[async_trait::async_trait]
    impl SessionQueryPort for NoopSessionQueries {
        async fn list_models(&self) -> Result<Vec<ModelSummary>, SdkError> {
            Ok(Vec::new())
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

    struct NoopConfigReader;

    #[async_trait::async_trait]
    impl ConfigReader for NoopConfigReader {
        fn committed_snapshot(&self) -> ConfigSnapshot {
            ConfigSnapshot::new(Default::default())
        }

        fn subscribe_committed(&self) -> tokio::sync::watch::Receiver<ConfigSnapshot> {
            let (sender, receiver) =
                tokio::sync::watch::channel(ConfigSnapshot::new(Default::default()));
            let _ = sender;
            receiver
        }

        async fn refresh_if_sources_changed(
            &self,
        ) -> Result<ConfigRefreshOutcomeData, DomainError> {
            Ok(ConfigRefreshOutcomeData::Unchanged)
        }

        async fn snapshot(&self) -> Result<ConfigSnapshot, DomainError> {
            Ok(ConfigSnapshot::new(Default::default()))
        }

        async fn subscribe(&self) -> Result<config::ConfigSubscriptionData, DomainError> {
            Err(DomainError::unavailable("config", "配置读取暂不可用"))
        }
    }
}

#[cfg(test)]
#[path = "immediate_query_tests.rs"]
mod tests;
