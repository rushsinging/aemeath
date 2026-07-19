use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

fn test_workspaces() -> &'static Mutex<
    HashMap<String, crate::application::tool_execution_adapters::RuntimeWorkspaceAccess>,
> {
    static WORKSPACES: OnceLock<
        Mutex<HashMap<String, crate::application::tool_execution_adapters::RuntimeWorkspaceAccess>>,
    > = OnceLock::new();
    WORKSPACES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn test_tool_execution_context(
    root: std::path::PathBuf,
    cancel: tokio_util::sync::CancellationToken,
) -> tools::ToolExecutionContext {
    let views = project::wire_production_workspace(root.clone())
        .expect("workspace initialization")
        .into_views();
    let workspace =
        crate::application::tool_execution_adapters::RuntimeWorkspaceAccess::new(views.clone());
    test_workspaces().lock().expect("test workspaces").insert(
        views.read().workspace_id().as_str().to_string(),
        workspace.clone(),
    );
    tools::ToolExecutionContext::new(
        tools::ExecutionScope::builder("test-run", views.read().workspace_id(), root).build(),
        tools::ToolExecutionPorts::new(
            crate::application::tool_execution_adapters::cancellation(cancel.clone()),
            workspace.read_access(),
            Arc::new(tools::MutexReadSet(Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )))),
            Arc::new(tools::FixedPlanMode(None)),
            Arc::new(memory::NoOpMemory),
            Arc::new(tools::FixedGuidance {
                language: "en".into(),
            }),
        ),
    )
}

pub(crate) fn runtime_workspace(
    ctx: &tools::ToolExecutionContext,
) -> crate::application::tool_execution_adapters::RuntimeWorkspaceAccess {
    test_workspaces()
        .lock()
        .expect("test workspaces")
        .get(ctx.scope().workspace_id().as_str())
        .expect("context workspace backing")
        .clone()
}

pub(crate) fn workspace_persist(
    ctx: &tools::ToolExecutionContext,
) -> Arc<dyn project::WorkspacePersist> {
    runtime_workspace(ctx).persist()
}

use async_trait::async_trait;
use futures::stream;
use provider::{
    InvocationDelta, InvocationEvent, InvocationStream, ProviderCompletion, ProviderContentBlock,
    ProviderStopReason, RawUsageSnapshot, ReasoningLevel,
};

pub(crate) fn test_tool_result_materializer(
) -> Arc<crate::application::tool_result_materialization::ToolResultMaterializer> {
    struct TestBlobPort;

    #[async_trait]
    impl crate::ports::ToolResultBlobPort for TestBlobPort {
        async fn write_once(
            &self,
            session_id: &str,
            tool_use_id: &str,
            _bytes: &[u8],
        ) -> Result<crate::ports::ToolResultBlobRef, crate::ports::ToolResultBlobError> {
            Ok(crate::ports::ToolResultBlobRef::new(format!(
                "tool-result://{session_id}/{tool_use_id}"
            )))
        }
    }

    Arc::new(
        crate::application::tool_result_materialization::ToolResultMaterializer::new(
            Arc::new(TestBlobPort),
            crate::application::tool_result_materialization::ToolResultMaterializationPolicy::new(
                50_000, 2_000, 500,
            ),
        ),
    )
}

pub(crate) fn text_completion_stream(
    text: impl Into<String>,
    input_tokens: u32,
    output_tokens: u32,
) -> InvocationStream {
    let text = text.into();
    Box::pin(stream::iter([
        InvocationEvent::Delta(InvocationDelta::Text(text.clone())),
        InvocationEvent::Completed(ProviderCompletion {
            output: vec![ProviderContentBlock::Text(text)],
            stop_reason: ProviderStopReason::EndTurn,
            usage: Some(RawUsageSnapshot {
                input_tokens: Some(input_tokens),
                output_tokens: Some(output_tokens),
                ..RawUsageSnapshot::default()
            }),
            effective_reasoning: ReasoningLevel::Off,
        }),
    ]))
}
