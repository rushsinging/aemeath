use super::change::RuntimeChange;
use super::intent::RuntimeIntent;
use super::processing_job::{ProcessingJob, ProcessingStatus};
use super::spinner::SpinnerModel;
use super::task_status::TaskStatusSnapshot;
use super::usage::UsageSummary;
use super::workspace::WorkspaceState;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeModel {
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub workspace: WorkspaceState,
    pub usage: UsageSummary,
    pub live_tps: Option<f64>,
    pub task_status: TaskStatusSnapshot,
    pub processing_jobs: Vec<ProcessingJob>,
    pub spinner: SpinnerModel,
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
                working_root,
                branch,
                kind,
            } => {
                self.workspace.path_base = path_base.clone();
                self.workspace.working_root = working_root.clone();
                self.workspace.branch = branch.clone();
                self.workspace.kind = kind;
                vec![RuntimeChange::WorkspaceSnapshotChanged {
                    path_base,
                    working_root,
                    branch,
                    kind,
                }]
            }
            RuntimeIntent::RecordUsage {
                input_tokens,
                output_tokens,
                cost_usd,
            } => {
                self.usage.input_tokens += input_tokens;
                self.usage.output_tokens += output_tokens;
                self.usage.cost_usd += cost_usd;
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
            RuntimeIntent::StartSpinner => {
                self.spinner.active = true;
                vec![RuntimeChange::SpinnerStarted]
            }
            RuntimeIntent::SetSpinnerPhase(phase) => {
                self.spinner.active = true;
                self.spinner.phase = Some(phase);
                vec![RuntimeChange::SpinnerPhaseChanged]
            }
            RuntimeIntent::StopSpinner => {
                self.spinner.active = false;
                self.spinner.phase = None;
                vec![RuntimeChange::SpinnerStopped]
            }
            RuntimeIntent::UpdateTaskLines(lines) => {
                self.task_status.lines = lines;
                vec![RuntimeChange::TaskLinesChanged]
            }
        }
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
            cost_usd: 0.01,
        });
        assert_eq!(model.usage.input_tokens, 10);
        assert_eq!(model.usage.output_tokens, 5);
    }

    #[test]
    fn test_runtime_start_spinner_sets_active() {
        let mut model = RuntimeModel::default();
        let changes = model.apply(RuntimeIntent::StartSpinner);
        assert!(model.spinner.active);
        assert!(matches!(
            changes.first(),
            Some(RuntimeChange::SpinnerStarted)
        ));
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
        let mut model = RuntimeModel::default();
        model.apply(RuntimeIntent::StartSpinner);
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
