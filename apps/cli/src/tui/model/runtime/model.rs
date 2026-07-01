use super::change::RuntimeChange;
use super::compact_progress::CompactProgressModel;
use super::intent::RuntimeIntent;
use super::processing_job::{ProcessingJob, ProcessingStatus};
use super::spinner::SpinnerModel;
use super::status_notice::StatusNotice;
use super::task_status::TaskStatusSnapshot;
use super::usage::UsageSummary;
use super::workspace::WorkspaceState;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeModel {
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub workspace: WorkspaceState,
    pub usage: UsageSummary,
    pub live_tps: Option<f64>,
    pub task_status: TaskStatusSnapshot,
    pub processing_jobs: Vec<ProcessingJob>,
    pub spinner: SpinnerModel,
    pub status_notice: StatusNotice,
    pub thinking: bool,
    /// Reasoning Graph 当前阶段（`None` = graph 不存在或 Idle），status notice 的持久真相。
    pub graph_phase: Option<String>,
    /// 临时 status notice 的过期时间戳；`None` 表示当前 notice 为持久态。
    /// 到期后由 SpinnerTick 回退到 `graph_phase` 派生的 notice。
    pub transient_notice_expiry: Option<Instant>,
    /// Compact 进度（`None` = 未在 compact 中），用于渲染 Gauge 进度条。
    pub compact_progress: Option<CompactProgressModel>,
}

impl Default for RuntimeModel {
    fn default() -> Self {
        Self {
            provider: None,
            model_id: None,
            workspace: WorkspaceState::default(),
            usage: UsageSummary::default(),
            live_tps: None,
            task_status: TaskStatusSnapshot::default(),
            processing_jobs: Vec::new(),
            spinner: SpinnerModel::default(),
            status_notice: StatusNotice::default(),
            thinking: true,
            graph_phase: None,
            transient_notice_expiry: None,
            compact_progress: None,
        }
    }
}

impl RuntimeModel {
    pub fn apply(&mut self, intent: RuntimeIntent) -> Vec<RuntimeChange> {
        match intent {
            RuntimeIntent::SetProviderModel { provider, model_id } => {
                self.provider = provider.clone();
                self.model_id = model_id.clone();
                vec![RuntimeChange::ProviderModelChanged { provider, model_id }]
            }
            RuntimeIntent::UpdateWorkspace { cwd, worktree } => {
                self.workspace.cwd = Some(cwd.clone());
                self.workspace.worktree = worktree.clone();
                vec![RuntimeChange::WorkspaceChanged { cwd, worktree }]
            }
            RuntimeIntent::WorkspaceSnapshotReceived {
                path_base,
                workspace_root,
                branch,
                kind,
            } => {
                self.workspace.path_base = path_base.clone();
                self.workspace.workspace_root = workspace_root.clone();
                self.workspace.branch = branch.clone();
                self.workspace.kind = kind;
                vec![RuntimeChange::WorkspaceSnapshotChanged {
                    path_base,
                    workspace_root,
                    branch,
                    kind,
                }]
            }
            RuntimeIntent::RecordUsage {
                input_tokens,
                output_tokens,
                last_input_tokens,
                cost_usd,
            } => {
                self.usage.input_tokens += input_tokens;
                self.usage.output_tokens += output_tokens;
                self.usage.last_input_tokens = last_input_tokens;
                self.usage.api_calls += 1;
                self.usage.cost_usd += cost_usd;
                vec![RuntimeChange::UsageChanged {
                    input_tokens: self.usage.input_tokens,
                    output_tokens: self.usage.output_tokens,
                    cost_usd: self.usage.cost_usd,
                }]
            }
            RuntimeIntent::SetContextSize(size) => {
                self.usage.context_size = size;
                vec![RuntimeChange::UsageChanged {
                    input_tokens: self.usage.input_tokens,
                    output_tokens: self.usage.output_tokens,
                    cost_usd: self.usage.cost_usd,
                }]
            }
            RuntimeIntent::UpdateLastInputTokens(tokens) => {
                self.usage.last_input_tokens = tokens;
                vec![RuntimeChange::UsageChanged {
                    input_tokens: self.usage.input_tokens,
                    output_tokens: self.usage.output_tokens,
                    cost_usd: self.usage.cost_usd,
                }]
            }
            RuntimeIntent::RecordLiveTps { tps } => {
                self.live_tps = Some(tps);
                vec![RuntimeChange::LiveTpsChanged { tps }]
            }
            RuntimeIntent::UpdateTaskStatus {
                total,
                completed,
                in_progress,
            } => {
                self.task_status = TaskStatusSnapshot {
                    total,
                    completed,
                    in_progress,
                    lines: std::mem::take(&mut self.task_status.lines),
                };
                vec![RuntimeChange::TaskStatusChanged {
                    total,
                    completed,
                    in_progress,
                }]
            }
            RuntimeIntent::StartProcessingJob { id, chat_id } => {
                self.processing_jobs.push(ProcessingJob {
                    id: id.clone(),
                    chat_id,
                    status: ProcessingStatus::Running,
                });
                vec![RuntimeChange::ProcessingJobChanged { id }]
            }
            RuntimeIntent::FinishProcessingJob { id, success } => {
                if let Some(job) = self.processing_jobs.iter_mut().find(|job| job.id == id) {
                    job.status = if success {
                        ProcessingStatus::Finished
                    } else {
                        ProcessingStatus::Failed
                    };
                }
                vec![RuntimeChange::ProcessingJobChanged { id }]
            }
            RuntimeIntent::SetSpinnerPhase(phase) => {
                self.spinner.active = true;
                self.spinner.phase = Some(phase);
                vec![RuntimeChange::SpinnerPhaseChanged]
            }
            RuntimeIntent::StopSpinner => {
                self.spinner.active = false;
                self.spinner.phase = None;
                self.compact_progress = None;
                vec![RuntimeChange::SpinnerStopped]
            }
            RuntimeIntent::UpdateTaskLines(lines) => {
                self.task_status.lines = lines;
                vec![RuntimeChange::TaskLinesChanged]
            }
            RuntimeIntent::SetStatusNotice(notice) => {
                self.status_notice = notice;
                self.transient_notice_expiry = None;
                vec![RuntimeChange::StatusNoticeChanged]
            }
            RuntimeIntent::SetTransientStatusNotice { notice, expires_at } => {
                self.status_notice = notice;
                self.transient_notice_expiry = Some(expires_at);
                vec![RuntimeChange::StatusNoticeChanged]
            }
            RuntimeIntent::SetThinking(enabled) => {
                self.thinking = enabled;
                vec![RuntimeChange::ThinkingChanged]
            }
            RuntimeIntent::SetGraphPhase(phase) => {
                self.graph_phase = phase.clone();
                // 非 transient 时同步更新 status_notice（持久真相单次写入）
                if self.transient_notice_expiry.is_none() {
                    self.status_notice = Self::notice_from_phase(phase.as_deref());
                }
                vec![RuntimeChange::GraphPhaseChanged]
            }
            RuntimeIntent::SetCompactProgress {
                stage,
                current,
                total,
            } => {
                self.compact_progress = Some(CompactProgressModel {
                    stage,
                    current,
                    total,
                });
                // #497：CompactProgress 是 compact 运行的原生信号。
                // spinner 启动不应依赖可选的 PreCompact hook（用户可能未配置），
                // 因此收到首个 CompactProgress 时自动启动 Compacting spinner，
                // 确保 Gauge 进度条可见。
                if !self.spinner.active {
                    self.spinner.active = true;
                    self.spinner.phase =
                        Some(crate::tui::model::runtime::spinner::SpinnerPhase::Compacting);
                }
                vec![RuntimeChange::SpinnerPhaseChanged]
            }
        }
    }

    /// 由 graph_phase 派生持久 status notice。
    fn notice_from_phase(phase: Option<&str>) -> StatusNotice {
        match phase {
            None | Some("idle") => StatusNotice::success("Ready"),
            Some(p) => StatusNotice::normal(p.to_string()),
        }
    }

    /// 检查临时 notice 是否过期；过期则回退到 graph_phase 派生的持久态。
    /// 返回 `true` 表示发生了回退（调用方可据此标脏）。
    pub fn expire_transient_notice(&mut self, now: Instant) -> bool {
        if self.transient_notice_expiry.is_some_and(|exp| now >= exp) {
            self.transient_notice_expiry = None;
            self.status_notice = Self::notice_from_phase(self.graph_phase.as_deref());
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::runtime::intent::RuntimeIntent;

    #[test]
    fn test_runtime_updates_workspace() {
        let mut model = RuntimeModel::default();
        let changes = model.apply(RuntimeIntent::UpdateWorkspace {
            cwd: "/repo".to_string(),
            worktree: Some("feature/x".to_string()),
        });
        assert_eq!(model.workspace.cwd.as_deref(), Some("/repo"));
        assert!(changes
            .iter()
            .any(|change| matches!(change, RuntimeChange::WorkspaceChanged { .. })));
    }

    #[test]
    fn test_runtime_records_usage() {
        let mut model = RuntimeModel::default();
        model.apply(RuntimeIntent::RecordUsage {
            input_tokens: 10,
            output_tokens: 5,
            last_input_tokens: 10,
            cost_usd: 0.01,
        });
        assert_eq!(model.usage.input_tokens, 10);
        assert_eq!(model.usage.output_tokens, 5);
        assert_eq!(model.usage.last_input_tokens, 10);
        assert_eq!(model.usage.api_calls, 1);
    }

    #[test]
    fn test_runtime_set_context_size() {
        let mut model = RuntimeModel::default();
        model.apply(RuntimeIntent::SetContextSize(200_000));

        assert_eq!(model.usage.context_size, 200_000);
    }

    #[test]
    fn test_runtime_set_spinner_phase_activates_and_sets() {
        use crate::tui::model::runtime::spinner::SpinnerPhase;
        let mut model = RuntimeModel::default();
        assert!(!model.spinner.active);
        let changes = model.apply(RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Thinking));
        assert!(model.spinner.active);
        assert_eq!(model.spinner.phase, Some(SpinnerPhase::Thinking));
        assert!(matches!(
            changes.first(),
            Some(RuntimeChange::SpinnerPhaseChanged)
        ));
    }

    #[test]
    fn test_runtime_stop_spinner_idempotent() {
        use crate::tui::model::runtime::spinner::SpinnerPhase;
        let mut model = RuntimeModel::default();
        model.apply(RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Thinking));
        model.apply(RuntimeIntent::StopSpinner);
        let changes = model.apply(RuntimeIntent::StopSpinner);
        assert!(!model.spinner.active);
        assert_eq!(model.spinner.phase, None);
        assert!(matches!(
            changes.first(),
            Some(RuntimeChange::SpinnerStopped)
        ));
    }

    #[test]
    fn test_runtime_update_task_lines() {
        let mut model = RuntimeModel::default();
        let changes = model.apply(RuntimeIntent::UpdateTaskLines(vec!["a".to_string()]));
        assert_eq!(model.task_status.lines, vec!["a".to_string()]);
        assert!(matches!(
            changes.first(),
            Some(RuntimeChange::TaskLinesChanged)
        ));
    }

    #[test]
    fn test_runtime_set_status_notice() {
        let mut model = RuntimeModel::default();
        let changes = model.apply(RuntimeIntent::SetStatusNotice(
            crate::tui::model::runtime::status_notice::StatusNotice::warning("Interrupted"),
        ));

        assert_eq!(model.status_notice.text, "Interrupted");
        assert_eq!(
            model.status_notice.kind,
            crate::tui::model::runtime::status_notice::StatusNoticeKind::Warning
        );
        assert!(matches!(
            changes.first(),
            Some(RuntimeChange::StatusNoticeChanged)
        ));
    }

    #[test]
    fn test_runtime_default_status_notice_is_ready() {
        let model = RuntimeModel::default();

        assert_eq!(model.status_notice.text, "Ready");
        assert_eq!(
            model.status_notice.kind,
            crate::tui::model::runtime::status_notice::StatusNoticeKind::Normal
        );
    }

    #[test]
    fn test_runtime_update_task_status_preserves_lines() {
        // 计数更新（UpdateTaskStatus）不得清空已设置的显示行（UpdateTaskLines）。
        let mut model = RuntimeModel::default();
        model.apply(RuntimeIntent::UpdateTaskLines(vec!["x".to_string()]));
        model.apply(RuntimeIntent::UpdateTaskStatus {
            total: 2,
            completed: 1,
            in_progress: 1,
        });
        assert_eq!(model.task_status.lines, vec!["x".to_string()]);
        assert_eq!(model.task_status.total, 2);
    }

    #[test]
    fn test_runtime_updates_task_status() {
        let mut model = RuntimeModel::default();
        let changes = model.apply(RuntimeIntent::UpdateTaskStatus {
            total: 3,
            completed: 1,
            in_progress: 2,
        });
        assert_eq!(model.task_status.total, 3);
        assert!(matches!(
            changes.first(),
            Some(RuntimeChange::TaskStatusChanged { in_progress, .. }) if *in_progress == 2
        ));
    }
}
