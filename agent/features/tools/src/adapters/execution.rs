//! Execution adapter with invocation-time registry, scope, profile, and schema checks.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::adapters::catalog::ToolBacking;
use crate::domain::published_language::{
    ToolErrorKind, ToolInvocation, ToolOutcome as ToolExecutionOutcome, ToolSuccess,
};
use crate::domain::scope_profile::is_authorized;
use crate::domain::{CancellationSignal, ExecutionScope, ToolExecutionContext};

trait ExecutionContextResolver: Send + Sync {
    fn resolve(&self, scope: &ExecutionScope) -> Option<ToolExecutionContext>;
}

/// Run-bound contexts are private adapter state; invocation PL never carries
/// resource ports or Runtime handles.
pub struct BoundExecutionContexts {
    by_run: RwLock<HashMap<String, ToolExecutionContext>>,
}

impl BoundExecutionContexts {
    pub fn new() -> Self {
        Self {
            by_run: RwLock::new(HashMap::new()),
        }
    }

    pub fn bind(&self, context: ToolExecutionContext) -> Result<(), String> {
        let run_id = context.scope().run_id().to_string();
        let mut contexts = self.by_run.write();
        if let Some(existing) = contexts.get(&run_id) {
            return if existing.scope() == context.scope() {
                Err(format!(
                    "tool execution context already bound for run {run_id}"
                ))
            } else {
                Err(format!(
                    "tool execution context scope conflict for run {run_id}"
                ))
            };
        }
        contexts.insert(run_id, context);
        Ok(())
    }

    pub fn unbind(&self, run_id: &str) {
        self.by_run.write().remove(run_id);
    }
}

impl Default for BoundExecutionContexts {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::domain::ToolExecutionContextBindingPort for BoundExecutionContexts {
    fn bind(&self, context: ToolExecutionContext) -> Result<(), String> {
        BoundExecutionContexts::bind(self, context)
    }

    fn unbind(&self, run_id: &str) {
        BoundExecutionContexts::unbind(self, run_id);
    }
}

impl ExecutionContextResolver for BoundExecutionContexts {
    fn resolve(&self, scope: &ExecutionScope) -> Option<ToolExecutionContext> {
        self.by_run
            .read()
            .get(scope.run_id())
            .filter(|context| context.scope() == scope)
            .cloned()
    }
}

pub struct ExecutionAdapter {
    backing: ToolBacking,
    contexts: Arc<dyn ExecutionContextResolver>,
}

impl ExecutionAdapter {
    pub fn new(backing: ToolBacking, contexts: Arc<BoundExecutionContexts>) -> Self {
        Self { backing, contexts }
    }

    async fn execute_checked(
        &self,
        invocation: ToolInvocation,
        cancellation: &dyn CancellationSignal,
    ) -> ToolExecutionOutcome {
        if cancellation.is_cancelled() {
            return ToolExecutionOutcome::cancelled("tool invocation cancelled before dispatch");
        }

        let scope = match self
            .backing
            .scope(invocation.execution_scope.registry_scope())
        {
            Some(scope) => scope,
            None => return unavailable(&invocation),
        };
        let profile = match self.backing.profile(invocation.execution_scope.profile()) {
            Some(profile) => profile,
            None => {
                return ToolExecutionOutcome::failure(
                    ToolErrorKind::Unauthorized,
                    "tool profile is not authorized",
                )
            }
        };
        let registration = match scope.get(&invocation.tool_name) {
            Some(registration) => registration,
            None => return unavailable(&invocation),
        };
        if !is_authorized(registration, profile) {
            return ToolExecutionOutcome::failure(
                ToolErrorKind::Unauthorized,
                "tool capabilities are not authorized by the selected profile",
            );
        }
        let tool = match self
            .backing
            .registry()
            .get(invocation.tool_name.normalized())
        {
            Some(tool) => tool,
            None => return unavailable(&invocation),
        };
        let context = match self.contexts.resolve(&invocation.execution_scope) {
            Some(context) => context,
            None => {
                return ToolExecutionOutcome::failure(
                    ToolErrorKind::ResourceUnavailable,
                    "tool execution context is unavailable for this run",
                )
            }
        };

        if let Err(mismatch) = crate::domain::schema_validator::validate_tool_input(
            tool.name(),
            &tool.input_schema(),
            &invocation.input,
        ) {
            return ToolExecutionOutcome::failure(
                ToolErrorKind::InvalidInput,
                crate::domain::schema_validator::format_tool_input_error(&mismatch),
            );
        }

        if let Some(suspension) = tool.suspension(&invocation.input) {
            return match suspension {
                Ok(value) => ToolExecutionOutcome::Suspended(value),
                Err(message) => ToolExecutionOutcome::failure(ToolErrorKind::InvalidInput, message),
            };
        }

        map_legacy_result(tool.call(invocation.input, &context).await)
    }
}

#[async_trait]
impl crate::domain::ToolExecutionPort for ExecutionAdapter {
    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: &dyn CancellationSignal,
    ) -> ToolExecutionOutcome {
        self.execute_checked(invocation, cancellation).await
    }
}

fn unavailable(invocation: &ToolInvocation) -> ToolExecutionOutcome {
    ToolExecutionOutcome::failure(
        ToolErrorKind::ToolUnavailable,
        format!("工具「{}」不存在或不在当前作用域内", invocation.tool_name),
    )
}

pub fn map_legacy_result(result: crate::domain::ToolResult) -> ToolExecutionOutcome {
    if result.is_error {
        ToolExecutionOutcome::failure(
            result.error_kind.unwrap_or(ToolErrorKind::InvalidInput),
            result.text,
        )
    } else {
        ToolExecutionOutcome::Success(ToolSuccess {
            content: vec![crate::domain::published_language::ContentBlock::text(
                result.text,
            )],
            data: (!result.data.is_null()).then_some(result.data),
            metadata: Default::default(),
        })
    }
}
