use crate::domain::{
    AgentDispatch, CancellationSignal, ExecutionScope, FixedGuidance, FixedPlanMode, MutexReadSet,
    ProgressSink, ToolExecutionContext, ToolExecutionPorts, WorkspaceReadAccess,
};
use async_trait::async_trait;
use project::{
    ProjectIdentity, WorkspaceControl, WorkspaceError, WorkspaceFrame, WorkspaceId, WorkspaceRead,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

/// Pure domain fake: it owns no runtime token or executor resource.
struct FakeCancellation;
#[async_trait]
impl CancellationSignal for FakeCancellation {
    fn is_cancelled(&self) -> bool {
        false
    }
    async fn cancelled(&self) {
        std::future::pending::<()>().await
    }
    fn child_signal(&self) -> Arc<dyn CancellationSignal> {
        Arc::new(Self)
    }
}

#[derive(Clone)]
struct FakeWorkspace {
    initial_root: PathBuf,
    current: Arc<Mutex<PathBuf>>,
}

impl FakeWorkspace {
    fn new(root: PathBuf) -> Self {
        let root = root.canonicalize().unwrap_or(root);
        Self {
            initial_root: root.clone(),
            current: Arc::new(Mutex::new(root)),
        }
    }

    fn current(&self) -> PathBuf {
        self.current.lock().expect("fake workspace lock").clone()
    }

    fn normalized(&self, path: &Path) -> PathBuf {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.current().join(path)
        };
        candidate.canonicalize().unwrap_or(candidate)
    }
}

impl WorkspaceRead for FakeWorkspace {
    fn workspace_id(&self) -> WorkspaceId {
        WorkspaceId::from(format!("test-workspace:{}", self.initial_root.display()))
    }
    fn project_identity(&self) -> ProjectIdentity {
        ProjectIdentity {
            initial_cwd: self.initial_root.display().to_string(),
            git_common_dir: None,
        }
    }
    fn current_workspace_root(&self) -> PathBuf {
        self.initial_root.clone()
    }
    fn current_path_base(&self) -> PathBuf {
        self.current()
    }
    fn resolve(&self, rel: &Path) -> PathBuf {
        self.normalized(rel)
    }
    fn resolve_file_path(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        let resolved = self.normalized(path);
        if resolved.starts_with(&self.initial_root) {
            Ok(resolved)
        } else {
            Err(WorkspaceError::PathOutsideWorkspaceRoot {
                path: resolved,
                root: self.initial_root.clone(),
            })
        }
    }
    fn resolve_search_path(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        let resolved = self.resolve_file_path(path)?;
        if !resolved.exists() {
            return Err(WorkspaceError::PathNotFound(resolved));
        }
        if !resolved.is_dir() {
            return Err(WorkspaceError::NotDirectory(resolved));
        }
        Ok(resolved)
    }
    fn in_worktree(&self) -> bool {
        false
    }
    fn current_branch(&self) -> Result<Option<String>, WorkspaceError> {
        Ok(None)
    }
    fn initial_cwd(&self) -> PathBuf {
        self.initial_root.clone()
    }
}

impl WorkspaceControl for FakeWorkspace {
    fn change_directory(&self, path: PathBuf) -> Result<(), WorkspaceError> {
        let resolved = self.normalized(&path);
        if !resolved.exists() {
            return Err(WorkspaceError::PathNotFound(resolved));
        }
        if !resolved.is_dir() {
            return Err(WorkspaceError::NotDirectory(resolved));
        }
        *self.current.lock().expect("fake workspace lock") = resolved;
        Ok(())
    }
    fn switch_to(&self, path: PathBuf) -> Result<(), WorkspaceError> {
        self.change_directory(path)
    }
    fn enter(
        &self,
        _path: Option<PathBuf>,
        _branch: Option<String>,
    ) -> Result<WorkspaceFrame, WorkspaceError> {
        Err(WorkspaceError::UnsupportedForNonGit)
    }
    fn exit(&self) -> Result<WorkspaceFrame, WorkspaceError> {
        Err(WorkspaceError::UnsupportedForNonGit)
    }
}

fn fake_workspace_read_access(workspace: FakeWorkspace) -> WorkspaceReadAccess {
    fake_controls()
        .lock()
        .expect("fake workspace controls")
        .insert(
            workspace.workspace_id().as_str().to_string(),
            Arc::new(workspace.clone()),
        );
    WorkspaceReadAccess::new(Arc::new(workspace))
}

fn fake_controls() -> &'static Mutex<HashMap<String, Arc<dyn WorkspaceControl>>> {
    static CONTROLS: OnceLock<Mutex<HashMap<String, Arc<dyn WorkspaceControl>>>> = OnceLock::new();
    CONTROLS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn workspace_control(ctx: &ToolExecutionContext) -> Arc<dyn WorkspaceControl> {
    fake_controls()
        .lock()
        .expect("fake workspace controls")
        .get(&ctx.scope().workspace_id().as_str().to_string())
        .expect("fake workspace control")
        .clone()
}

/// Purpose-built, runtime-free fixture for domain and ordinary adapter unit tests.
pub(crate) struct TestToolExecutionContextBuilder {
    root: PathBuf,
    allow_all: bool,
    read_files: HashSet<String>,
    agent: Option<Arc<dyn AgentDispatch>>,
    progress: Option<Arc<dyn ProgressSink>>,
}

impl TestToolExecutionContextBuilder {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            allow_all: false,
            read_files: HashSet::new(),
            agent: None,
            progress: None,
        }
    }
    pub(crate) fn allow_all(mut self, value: bool) -> Self {
        self.allow_all = value;
        self
    }
    pub(crate) fn read_file(mut self, path: impl Into<String>) -> Self {
        self.read_files.insert(path.into());
        self
    }
    pub(crate) fn agent(mut self, agent: Arc<dyn AgentDispatch>) -> Self {
        self.agent = Some(agent);
        self
    }
    pub(crate) fn progress_sink(mut self, sink: Arc<dyn ProgressSink>) -> Self {
        self.progress = Some(sink);
        self
    }
    pub(crate) fn build(self) -> ToolExecutionContext {
        let workspace = FakeWorkspace::new(self.root);
        let scope = ExecutionScope::builder(
            "test-run",
            workspace.workspace_id(),
            workspace.current_workspace_root(),
        )
        .build();
        let ports = ToolExecutionPorts::new(
            Arc::new(FakeCancellation),
            fake_workspace_read_access(workspace),
            Arc::new(MutexReadSet(Arc::new(Mutex::new(self.read_files)))),
            Arc::new(FixedPlanMode(None)),
            Arc::new(memory::NoOpMemory),
            Arc::new(FixedGuidance {
                language: "en".into(),
                allow_all: self.allow_all,
            }),
        )
        .with_agent(self.agent)
        .with_progress(self.progress);
        ToolExecutionContext::new(scope, ports)
    }
}
