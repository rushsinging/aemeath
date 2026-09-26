use task::{TaskCommandResultData, TaskEventData, TaskRevisionData, TaskStatusData};

/// Runtime-only fact that a TaskData command committed state.
///
/// This type intentionally carries no TaskData aggregate, display text, or wire data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedTaskChange {
    revision: TaskRevisionData,
    facts: Vec<TaskChangeFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskChangeFact {
    Created { task_id: task::TaskIdData },
    Completed { task_id: task::TaskIdData },
}

impl CommittedTaskChange {
    pub fn from_command_result<T>(result: &TaskCommandResultData<T>) -> Option<Self> {
        let revision = result.revision()?;
        let facts = result
            .events
            .iter()
            .filter_map(|event| match event {
                TaskEventData::TaskCreated { task_id } => {
                    Some(TaskChangeFact::Created { task_id: *task_id })
                }
                TaskEventData::TaskStatusChanged { task_id, to, .. }
                    if *to == TaskStatusData::Completed =>
                {
                    Some(TaskChangeFact::Completed { task_id: *task_id })
                }
                _ => None,
            })
            .collect();
        Some(Self { revision, facts })
    }

    pub fn revision(&self) -> TaskRevisionData {
        self.revision
    }

    pub fn facts(&self) -> &[TaskChangeFact] {
        &self.facts
    }
}
