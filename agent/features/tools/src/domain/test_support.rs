use crate::domain::{
    AgentDispatch, CancellationSignal, ExecutionScope, FixedGuidance, MutexReadSet, ProgressSink,
    ToolExecutionContext, ToolExecutionPorts, WorkspaceReadAccess,
};
use async_trait::async_trait;
use project::{WorkspaceControl, WorkspaceReader};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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

fn fake_workspace_read_access(
    workspace: project::test_support::FakeWorkspace,
) -> WorkspaceReadAccess {
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

/// bash adapter 内联测试专用 fake（仅 cfg(test) 编译；test-harness feature
/// 的 lib 侧消费者不需要它，加 cfg 防 dead_code）。
#[cfg(test)]
pub(crate) fn workspace_control(ctx: &ToolExecutionContext) -> Arc<dyn WorkspaceControl> {
    fake_controls()
        .lock()
        .expect("fake workspace controls")
        .get(&ctx.scope().workspace_id().as_str().to_string())
        .expect("fake workspace control")
        .clone()
}

/// Purpose-built, runtime-free fixture for domain and ordinary adapter unit tests.
pub struct TestToolExecutionContextBuilder {
    root: PathBuf,
    allow_all: bool,
    read_files: HashSet<String>,
    agent: Option<Arc<dyn AgentDispatch>>,
    progress: Option<Arc<dyn ProgressSink>>,
}

impl TestToolExecutionContextBuilder {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            allow_all: false,
            read_files: HashSet::new(),
            agent: None,
            progress: None,
        }
    }
    pub fn allow_all(mut self, value: bool) -> Self {
        self.allow_all = value;
        self
    }
    pub fn read_file(mut self, path: impl Into<String>) -> Self {
        self.read_files.insert(path.into());
        self
    }
    pub fn agent(mut self, agent: Arc<dyn AgentDispatch>) -> Self {
        self.agent = Some(agent);
        self
    }
    pub fn progress_sink(mut self, sink: Arc<dyn ProgressSink>) -> Self {
        self.progress = Some(sink);
        self
    }
    pub fn build(self) -> ToolExecutionContext {
        let authorization = if self.allow_all {
            crate::domain::context::AuthorizationContext::ALLOW_ALL
        } else {
            crate::domain::context::AuthorizationContext::STANDARD
        };
        let workspace = project::test_support::FakeWorkspace::new(self.root);
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

/// 测试用单工具 Catalog：sequential / NonCooperative / 指定 timeout。
pub fn sequential_test_tool_catalog(
    tool_name: &str,
    timeout_secs: u64,
) -> crate::ToolCatalogSnapshot {
    let descriptor = crate::domain::published_language::ToolDescriptor {
        name: crate::ToolName::new(tool_name),
        description: format!("test tool {tool_name}"),
        input_schema: serde_json::json!({}),
        required_capabilities: crate::ToolCapabilities::empty(),
        concurrency: crate::domain::published_language::ConcurrencyDeclaration::serialized(),
        cancellation: crate::domain::published_language::CancellationDeclaration::NonCooperative,
        timeout_secs,
        max_timeout_secs: crate::domain::constants::DEFAULT_TOOL_MAX_TIMEOUT_SECS,
        read_only: true,
        input_safety: crate::domain::published_language::InputSafetyDeclaration::Always,
        data_schema: serde_json::json!({}),
        background_log_direct: false,
    };
    crate::ToolCatalogSnapshot::new("main", "standard", vec![descriptor])
}

/// 固定描述符集的 SkillCatalogPort 替身（#2146：实现权归 tools crate）。
pub struct FixedSkillCatalog(pub Vec<crate::domain::skill_pl::SkillDescriptor>);

impl crate::domain::skill_ports::SkillCatalogPort for FixedSkillCatalog {
    fn list(
        &self,
        _query: crate::domain::skill_pl::SkillQuery,
    ) -> Vec<crate::domain::skill_pl::SkillDescriptor> {
        self.0.clone()
    }
}

// #2146 密封解锁。
impl crate::sealed::Sealed for FixedSkillCatalog {}

/// 可变描述符集替身（共享 Mutex，模拟轮次间磁盘变化）。
pub struct MutableSkillCatalog(
    pub std::sync::Arc<std::sync::Mutex<Vec<crate::domain::skill_pl::SkillDescriptor>>>,
);

impl crate::domain::skill_ports::SkillCatalogPort for MutableSkillCatalog {
    fn list(
        &self,
        _query: crate::domain::skill_pl::SkillQuery,
    ) -> Vec<crate::domain::skill_pl::SkillDescriptor> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}
impl crate::sealed::Sealed for MutableSkillCatalog {}
