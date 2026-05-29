#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TaskStatusSnapshot {
    pub total: usize,
    pub completed: usize,
    pub in_progress: usize,
    pub lines: Vec<String>,
}
