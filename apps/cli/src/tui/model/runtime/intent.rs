#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeIntent {
    UpdateWorkspace {
        cwd: String,
        worktree: Option<String>,
    },
    RecordUsage {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },
    UpdateTaskStatus {
        total: usize,
        completed: usize,
        in_progress: usize,
    },
}
