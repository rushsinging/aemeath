use super::change::RuntimeChange;
use super::intent::RuntimeIntent;
use super::task_status::TaskStatusSnapshot;
use super::usage::UsageSummary;
use super::workspace::WorkspaceState;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeModel {
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub workspace: WorkspaceState,
    pub usage: UsageSummary,
    pub task_status: TaskStatusSnapshot,
}

impl RuntimeModel {
    pub fn apply(&mut self, intent: RuntimeIntent) -> Vec<RuntimeChange> {
        match intent {
            RuntimeIntent::UpdateWorkspace { cwd, worktree } => {
                self.workspace.cwd = Some(cwd.clone());
                self.workspace.worktree = worktree.clone();
                vec![RuntimeChange::WorkspaceChanged { cwd, worktree }]
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
