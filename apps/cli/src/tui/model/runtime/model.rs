use super::change::RuntimeChange;
use super::intent::RuntimeIntent;
use super::processing_job::{ProcessingJob, ProcessingStatus};
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
