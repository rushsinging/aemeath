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
    /// 过程事件记录器（#2070：周期全链路事实落盘，评测输入）。
    recorder: Option<std::sync::Arc<crate::domain::events::RsiEventRecorder>>,
    /// 组件版本快照库（#2074：采纳前备份，回滚依据）。
    version_store: Option<std::sync::Arc<crate::domain::versioning::ComponentVersionStore>>,
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
            recorder: None,
            version_store: None,
        }
    }

    /// 注入组件版本快照库（#2074）。
    pub fn with_version_store(
        mut self,
        store: std::sync::Arc<crate::domain::versioning::ComponentVersionStore>,
    ) -> Self {
        self.version_store = Some(store);
        self
    }

    /// 注入过程事件记录器（#2070）。
    pub fn with_event_recorder(
        mut self,
        recorder: std::sync::Arc<crate::domain::events::RsiEventRecorder>,
    ) -> Self {
        self.recorder = Some(recorder);
        self
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
        let cycle_started = std::time::Instant::now();
        let cycle_id = format!(
            "cycle-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or(0)
        );
        if let Some(recorder) = &self.recorder {
            recorder.record(&crate::domain::events::RsiCycleEvent::CycleStart {
                cycle_id: cycle_id.clone(),
                trigger: "manual".to_string(),
                model: String::new(),
                dry_run: self.dry_run,
                blocks: blocks.len(),
            });
        }
        let cursor_start = self.stream_port.cursor_summary();
        let mut summary_parts = Vec::new();
        let mut preview_lines = Vec::new();
        let mut report_lines = Vec::new();
        let mut all_adoptions: Vec<crate::domain::task::AdoptionRecord> = Vec::new();

        // #2078 修订：诊断优先模式——每块 1 次诊断 + 跨块汇总，不落盘。
        if self.config.execution_mode == share::config::RsiExecutionMode::DiagnoseOnly {
            let lessons = self.lesson_store.load_active().unwrap_or_default();
            let mut diagnosis_report = Vec::new();
            let mut merged_evidence = crate::domain::evidence::EvidenceBundle::default();
            for block in &blocks {
                let evidence = self.stream_port.block_evidence(block);
                merged_evidence.signals.extend(evidence.signals);
                merged_evidence.sessions_covered += evidence.sessions_covered;
            }
            let summary = crate::domain::improvement::run_diagnosis_cycle(
                registry,
                self.llm.as_ref(),
                &self.config,
                merged_evidence,
                &lessons,
                self.recorder.as_deref(),
                &mut diagnosis_report,
            )
            .await?;
            let prefix = if self.dry_run { "（dry-run）" } else { "" };
            let mut summary_text = format!(
                "{prefix}诊断完成：{} 条任务（不改动组件）",
                summary.issues.len()
            );
            if !diagnosis_report.is_empty() {
                summary_text.push_str("\n（诊断报告）\n");
                summary_text.push_str(&diagnosis_report.join("\n"));
            }
            return Ok(crate::domain::task::CycleOutcome {
                summary: summary_text,
                adopted: Vec::new(),
            });
        }
        if self.dry_run {
            report_lines.push(format!(
                "—— checkpoint：起始 {}（dry-run NEVER 推进）",
                cursor_start.as_deref().unwrap_or("无游标（首次周期）")
            ));
            report_lines.push(format!("—— 数据源分块：{} 块", blocks.len()));
        }
        for (block_index, block) in blocks.iter().enumerate() {
            let evidence = self.stream_port.block_evidence(block);
            if let Some(recorder) = &self.recorder {
                recorder.record(&crate::domain::events::RsiCycleEvent::BlockEvidence {
                    block_index,
                    sessions_runs: block
                        .runs
                        .iter()
                        .map(|run| format!("{}#{}", run.session_key, run.run_id))
                        .collect(),
                    signals: evidence
                        .signals
                        .iter()
                        .map(|signal| signal.signal_id.clone())
                        .collect(),
                });
            }
            if self.dry_run {
                let sessions: Vec<String> = block
                    .runs
                    .iter()
                    .map(|run| format!("{}#{}", short_key(&run.session_key), run.run_id))
                    .collect();
                report_lines.push(format!(
                    "  块 {}/{}：{}",
                    block_index + 1,
                    blocks.len(),
                    sessions.join("，")
                ));
                let signals: Vec<String> = evidence
                    .signals
                    .iter()
                    .map(|signal| signal.signal_id.clone())
                    .collect();
                report_lines.push(format!(
                    "    证据：{}",
                    if signals.is_empty() {
                        "（无信号）".to_string()
                    } else {
                        signals.join(" | ")
                    }
                ));
            }
            let outcome = run_improvement_cycle(
                registry,
                self.llm.as_ref(),
                &self.lesson_store,
                &self.config,
                evidence,
                self.dry_run,
                &mut preview_lines,
                self.recorder.as_deref(),
                self.version_store.as_deref(),
            )
            .await?;
            if self.dry_run {
                report_lines.push(format!("    提案结果：{}", outcome.summary));
            }
            summary_parts.push(outcome.summary);
            all_adoptions.extend(outcome.adopted);
            if !self.dry_run {
                let fingerprint = crate::domain::stream_port::component_tree_fingerprint(registry);
                self.stream_port.advance(block, fingerprint);
            }
        }
        // #2065 A：周期 Compose 汇总块——纵观全部采纳仲裁矛盾/合并/精炼
        if !all_adoptions.is_empty() {
            if let Ok(Some(compose_outcome)) = crate::domain::improvement::run_compose_phase(
                registry,
                self.llm.as_ref(),
                &self.lesson_store,
                &all_adoptions,
                self.dry_run,
                &mut preview_lines,
                self.recorder.as_deref(),
                self.version_store.as_deref(),
            )
            .await
            {
                if self.dry_run {
                    report_lines.push(format!("    Compose：{}", compose_outcome.summary));
                }
                summary_parts.push(compose_outcome.summary);
            }
        }
        if summary_parts.is_empty() {
            return Ok(crate::domain::task::CycleOutcome {
                summary: "无待处理证据块".to_string(),
                adopted: Vec::new(),
            });
        }
        let mut summary = summary_parts.join(" | ");
        if self.dry_run {
            summary.push_str("\n（dry-run 报告）\n");
            summary.push_str(&report_lines.join("\n"));
            if !preview_lines.is_empty() {
                summary.push_str("\n—— 组件落盘结果（预演，零写入）：\n");
                summary.push_str(&preview_lines.join("\n"));
            }
        }
        if let Some(recorder) = &self.recorder {
            recorder.record(&crate::domain::events::RsiCycleEvent::CycleEnd {
                summary: summary.chars().take(200).collect(),
                duration_ms: cycle_started.elapsed().as_millis() as u64,
            });
        }
        Ok(crate::domain::task::CycleOutcome {
            summary,
            adopted: all_adoptions,
        })
    }
}

/// session key 短形态（截 UUID 前段，报告可读）。
fn short_key(session_key: &str) -> String {
    session_key
        .strip_suffix(".dataset")
        .unwrap_or(session_key)
        .split('-')
        .next()
        .unwrap_or(session_key)
        .to_string()
}
