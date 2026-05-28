#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeChange {
    WorkspaceChanged {
        cwd: String,
        worktree: Option<String>,
    },
    UsageChanged {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },
    TaskStatusChanged {
        total: usize,
        completed: usize,
        in_progress: usize,
    },
}
