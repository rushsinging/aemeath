//! tools 测试替身（#2146 治理后统一由 testkit 维护）。
//!
//! ToolExecutionPort / SkillCatalogPort / CancellationSignal 等实现权归 tools crate，
//! 跨 crate 测试经本模块获得替身；消费方经 `[dev-dependencies] testkit` 引用，
//! 不进生产依赖图。

use async_trait::async_trait;
use project::{WorkspaceControl, WorkspaceReader};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use tools::published::agent::AgentDispatch;
use tools::published::execution::{
    CancellationSignal, ProgressSink, ToolExecutionContext, ToolExecutionPorts,
};
use tools::published::skill::{SkillCatalogPort, SkillDescriptor, SkillQuery};
use tools::{
    AuthorizationContext, CancellationDeclaration, ConcurrencyDeclaration, ExecutionScope,
    FixedGuidance, InputSafetyDeclaration, MutexReadSet, ToolCapabilities, ToolCatalogSnapshot,
    ToolDescriptor, ToolName, WorkspaceReadAccess,
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

fn fake_workspace_read_access(workspace: crate::project::FakeWorkspace) -> WorkspaceReadAccess {
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
            AuthorizationContext::ALLOW_ALL
        } else {
            AuthorizationContext::STANDARD
        };
        let workspace = crate::project::FakeWorkspace::new(self.root);
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
pub fn sequential_test_tool_catalog(tool_name: &str, timeout_secs: u64) -> ToolCatalogSnapshot {
    let descriptor = ToolDescriptor {
        name: ToolName::new(tool_name),
        description: format!("test tool {tool_name}"),
        input_schema: serde_json::json!({}),
        required_capabilities: ToolCapabilities::empty(),
        concurrency: ConcurrencyDeclaration::serialized(),
        cancellation: CancellationDeclaration::NonCooperative,
        timeout_secs,
        // 与生产 DEFAULT_TOOL_MAX_TIMEOUT_SECS（86400）等值：timeout 上限 clamp
        // 语义是既有测试断言的一部分，故不随本次 timeout_secs 变化。
        max_timeout_secs: 86_400,
        read_only: true,
        input_safety: InputSafetyDeclaration::Always,
        data_schema: serde_json::json!({}),
        background_log_direct: false,
    };
    ToolCatalogSnapshot::new("main", "standard", vec![descriptor])
}

/// 固定描述符集的 SkillCatalogPort 替身（）。
pub struct FixedSkillCatalog(pub Vec<SkillDescriptor>);

impl SkillCatalogPort for FixedSkillCatalog {
    fn list(&self, _query: SkillQuery) -> Vec<SkillDescriptor> {
        self.0.clone()
    }
}

/// 可变描述符集替身（共享 Mutex，模拟轮次间磁盘变化）。
pub struct MutableSkillCatalog(pub std::sync::Arc<std::sync::Mutex<Vec<SkillDescriptor>>>);

impl SkillCatalogPort for MutableSkillCatalog {
    fn list(&self, _query: SkillQuery) -> Vec<SkillDescriptor> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

/// 工具执行替身：
/// execute 按脚本返回 outcome（不可 await；需要 sleep/取消交互的测试
/// 用 `scripted_tool_execution_async`）。
pub fn scripted_tool_execution(
    script: impl Fn(
            &tools::published::execution::ToolInvocation,
            &ToolExecutionContext,
        ) -> tools::published::execution::ToolExecutionOutcome
        + Send
        + Sync
        + 'static,
) -> std::sync::Arc<dyn tools::published::execution::ToolExecutionPort> {
    std::sync::Arc::new(ScriptedToolExecution {
        script: std::sync::Arc::new(script),
    })
}

/// 工具执行脚本（同步形态）。
type SyncToolScript = dyn Fn(
        &tools::published::execution::ToolInvocation,
        &ToolExecutionContext,
    ) -> tools::published::execution::ToolExecutionOutcome
    + Send
    + Sync;

struct ScriptedToolExecution {
    script: std::sync::Arc<SyncToolScript>,
}

#[async_trait]
impl tools::published::execution::ToolExecutionPort for ScriptedToolExecution {
    async fn execute(
        &self,
        invocation: tools::published::execution::ToolInvocation,
        context: &ToolExecutionContext,
    ) -> tools::published::execution::ToolExecutionOutcome {
        (self.script)(&invocation, context)
    }
}

/// 异步工具执行替身：脚本返回 Future（sleep / 取消交互场景）。
pub fn scripted_tool_execution_async<F, Fut>(
    script: F,
) -> std::sync::Arc<dyn tools::published::execution::ToolExecutionPort>
where
    F: for<'a> Fn(tools::published::execution::ToolInvocation, &'a ToolExecutionContext) -> Fut
        + Send
        + Sync
        + 'static,
    Fut: std::future::Future<Output = tools::published::execution::ToolExecutionOutcome> + Send,
{
    std::sync::Arc::new(AsyncScriptedToolExecution {
        script: std::sync::Arc::new(script),
    })
}

struct AsyncScriptedToolExecution<F> {
    script: std::sync::Arc<F>,
}

#[async_trait]
impl<F, Fut> tools::published::execution::ToolExecutionPort for AsyncScriptedToolExecution<F>
where
    F: for<'a> Fn(tools::published::execution::ToolInvocation, &'a ToolExecutionContext) -> Fut
        + Send
        + Sync,
    Fut: std::future::Future<Output = tools::published::execution::ToolExecutionOutcome> + Send,
{
    async fn execute(
        &self,
        invocation: tools::published::execution::ToolInvocation,
        context: &ToolExecutionContext,
    ) -> tools::published::execution::ToolExecutionOutcome {
        (self.script)(invocation, context).await
    }
}
