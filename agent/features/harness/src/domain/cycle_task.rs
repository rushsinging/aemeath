//! HarnessRsi 任务：RsiTask 实现，封装改进循环（registry + LLM 端口注入）。

use crate::application::ComponentRegistry;
use crate::domain::errors::HarnessError;
use crate::domain::improvement::{run_improvement_cycle, RsiLlmPort};
use crate::domain::lesson::LessonStore;
use crate::domain::task::{CycleOutcome, RsiTask};
use async_trait::async_trait;
use share::config::RsiConfig;
use std::sync::Arc;
use tokio::sync::Mutex;

/// HarnessRsi 改进任务（daemon 周期内执行）。
///
/// registry 与 LLM 端口经装配注入（composition 组合根）；本类型只做
/// 「持配置 + 委托改进循环」——领域逻辑全部在 improvement 模块。
pub struct HarnessRsiCycleTask {
    registry: Arc<Mutex<Box<dyn ComponentRegistry + Send>>>,
    llm: Arc<dyn RsiLlmPort>,
    lesson_store: Arc<LessonStore>,
    config: RsiConfig,
    /// 执行证据装载器（装配层扫描 session dataset；#2049 流模型
    /// 接入前为「最新 N dataset 聚合」形态）。
    evidence_loader: Box<dyn Fn() -> crate::domain::evidence::EvidenceBundle + Send>,
}

impl HarnessRsiCycleTask {
    pub fn new(
        registry: Arc<Mutex<Box<dyn ComponentRegistry + Send>>>,
        llm: Arc<dyn RsiLlmPort>,
        lesson_store: Arc<LessonStore>,
        config: RsiConfig,
    ) -> Self {
        Self {
            registry,
            llm,
            lesson_store,
            config,
            evidence_loader: Box::new(crate::domain::evidence::EvidenceBundle::default),
        }
    }

    /// 注入执行证据装载器（#2048）。
    pub fn with_evidence_loader(
        mut self,
        loader: Box<dyn Fn() -> crate::domain::evidence::EvidenceBundle + Send>,
    ) -> Self {
        self.evidence_loader = loader;
        self
    }
}

#[async_trait]
impl RsiTask for HarnessRsiCycleTask {
    async fn run_cycle(&mut self) -> Result<CycleOutcome, HarnessError> {
        let registry = &mut **self.registry.lock().await;
        let evidence = (self.evidence_loader)();
        run_improvement_cycle(
            registry,
            self.llm.as_ref(),
            &self.lesson_store,
            &self.config,
            evidence,
        )
        .await
    }
}
