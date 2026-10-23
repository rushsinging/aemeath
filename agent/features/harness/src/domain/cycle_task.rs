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
    /// 证据流端口（#2049 流模型：plan → block_evidence → advance）。
    stream_port: Box<dyn crate::domain::stream_port::EvidenceStreamPort>,
    /// 预演模式（#2050）：完整周期但零持久副作用。
    dry_run: bool,
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
            stream_port: Box::new(crate::domain::stream_port::FakeEvidenceStream::default()),
            dry_run: false,
        }
    }

    /// 预演模式（#2050）：组件/lesson/checkpoint 全不写。
    pub fn with_dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    /// 注入证据流端口（#2049 流模型）。
    pub fn with_evidence_stream(
        mut self,
        stream_port: Box<dyn crate::domain::stream_port::EvidenceStreamPort>,
    ) -> Self {
        self.stream_port = stream_port;
        self
    }
}

#[async_trait]
impl RsiTask for HarnessRsiCycleTask {
    async fn run_cycle(&mut self) -> Result<CycleOutcome, HarnessError> {
        let registry = &mut **self.registry.lock().await;
        // 数据源块驱动（#2049）：每块独立「证据 → 提案 → 采纳 → lesson」，
        // 块成功（周期 Ok——作废也是明确处理结论）才推进游标。
        let params = crate::domain::stream::StreamPlanParams {
            initial_window_days: self.config.evidence_initial_window_days,
            max_runs_per_cycle: self.config.evidence_max_sessions_per_cycle.max(1),
            block_run_limit: self.config.evidence_block_sessions.max(1),
            component_tree_fingerprint: String::new(),
        };
        let blocks = self.stream_port.plan(&params);
        let mut summary_parts = Vec::new();
        let mut preview_lines = Vec::new();
        for block in &blocks {
            let evidence = self.stream_port.block_evidence(block);
            let outcome = run_improvement_cycle(
                registry,
                self.llm.as_ref(),
                &self.lesson_store,
                &self.config,
                evidence,
                self.dry_run,
                &mut preview_lines,
            )
            .await?;
            summary_parts.push(outcome.summary);
            if !self.dry_run {
                let fingerprint = crate::domain::stream_port::component_tree_fingerprint(registry);
                self.stream_port.advance(block, fingerprint);
            }
        }
        if summary_parts.is_empty() {
            return Ok(crate::domain::task::CycleOutcome {
                summary: "无待处理证据块".to_string(),
            });
        }
        let mut summary = summary_parts.join(" | ");
        if self.dry_run && !preview_lines.is_empty() {
            summary.push_str("\n（dry-run 预览）\n");
            summary.push_str(&preview_lines.join("\n"));
        }
        Ok(crate::domain::task::CycleOutcome { summary })
    }
}
