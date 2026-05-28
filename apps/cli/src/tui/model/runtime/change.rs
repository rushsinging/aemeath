use super::workspace::WorktreeKind;

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeChange {
    ProviderModelChanged {
        provider: Option<String>,
        model_id: Option<String>,
    },
    WorkspaceChanged {
        cwd: String,
        worktree: Option<String>,
    },
    WorkspaceSnapshotChanged {
        path_base: Option<String>,
        working_root: Option<String>,
        branch: Option<String>,
        kind: WorktreeKind,
    },
    UsageChanged {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },
    LiveTpsChanged {
        tps: f64,
    },
    TaskStatusChanged {
        total: usize,
        completed: usize,
        in_progress: usize,
    },
    ProcessingJobChanged {
        id: String,
    },
}
