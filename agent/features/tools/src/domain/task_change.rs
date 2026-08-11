use task::{TaskCommandResult, TaskEvent, TaskRevision, TaskStatus};

/// Runtime-only fact that a Task command committed state.
///
/// This type intentionally carries no Task aggregate, display text, or wire data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedTaskChange {
    revision: TaskRevision,
    facts: Vec<TaskChangeFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskChangeFact {
    Created { task_id: task::TaskId },
    Completed { task_id: task::TaskId },
}

impl CommittedTaskChange {
    pub fn from_command_result<T>(result: &TaskCommandResult<T>) -> Option<Self> {
        let revision = result.revision()?;
        let facts = result
            .events
            .iter()
            .filter_map(|event| match event {
                TaskEvent::TaskCreated { task_id } => {
                    Some(TaskChangeFact::Created { task_id: *task_id })
                }
                TaskEvent::TaskStatusChanged { task_id, to, .. }
                    if *to == TaskStatus::Completed =>
                {
                    Some(TaskChangeFact::Completed { task_id: *task_id })
                }
                _ => None,
            })
            .collect();
        Some(Self { revision, facts })
    }

    pub fn revision(&self) -> TaskRevision {
        self.revision
    }

    pub fn facts(&self) -> &[TaskChangeFact] {
        &self.facts
    }
}
