//! tools 域测试替身（crate 内 lib 测试专用；跨 crate 共享的 tools fake 在
//! `testkit::tools` 维护——本模块不对外导出）。
//!
//! Cargo dev-dependency 循环下，被测试 crate 自身经 testkit 消费自己的 fake
//! 会产生双实例类型不匹配，故 crate 内测试专用 fixture 留在本模块。

use async_trait::async_trait;
use project::{WorkspaceControl, WorkspaceReader};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

use crate::domain::skill_pl::{SkillDescriptor, SkillQuery};
use crate::domain::skill_ports::SkillCatalogPort;
use crate::domain::{
    AgentDispatch, CancellationSignal, ExecutionScope, FixedGuidance, MutexReadSet, ProgressSink,
    ToolExecutionContext, ToolExecutionPorts, WorkspaceReadAccess,
};

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

fn fake_workspace_read_access(workspace: testkit::project::FakeWorkspace) -> WorkspaceReadAccess {
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

/// bash adapter 内联测试专用 fake：按 ctx 的 workspace_id 取注册过的 control。
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
        let authorization = if self.allow_all {
            crate::domain::context::AuthorizationContext::ALLOW_ALL
        } else {
            crate::domain::context::AuthorizationContext::STANDARD
        };
        let workspace = testkit::project::FakeWorkspace::new(self.root);
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
            Arc::new(memory::api::NoOpMemory),
            Arc::new(FixedGuidance {
                language: "en".into(),
            }),
        )
        .with_agent(self.agent)
        .with_progress(self.progress);
        ToolExecutionContext::new(scope, ports).with_authorization(authorization)
    }
}

/// 固定描述符集的 SkillCatalogPort 替身（crate 内测试专用）。
pub(crate) struct FixedSkillCatalog(pub Vec<SkillDescriptor>);

impl SkillCatalogPort for FixedSkillCatalog {
    fn list(&self, _query: SkillQuery) -> Vec<SkillDescriptor> {
        self.0.clone()
    }
}
