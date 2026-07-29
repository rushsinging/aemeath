//! Execution adapter with invocation-time registry, scope, profile, and schema checks.

use async_trait::async_trait;

use crate::adapters::catalog::ToolBacking;
use crate::domain::published_language::{
    ToolErrorKind, ToolInvocation, ToolOutcome as ToolExecutionOutcome, ToolSuccess,
};
use crate::domain::scope_profile::is_authorized;
use crate::domain::ToolExecutionContext;

pub struct ExecutionAdapter {
    backing: ToolBacking,
}

impl ExecutionAdapter {
    pub fn new(backing: ToolBacking) -> Self {
        Self { backing }
    }

    async fn execute_checked(
        &self,
        invocation: ToolInvocation,
        context: &ToolExecutionContext,
    ) -> ToolExecutionOutcome {
        if invocation.execution_scope != *context.scope() {
            return ToolExecutionOutcome::failure(
                ToolErrorKind::ResourceUnavailable,
                "tool execution context does not match invocation scope",
            );
        }
        if context.cancellation().is_cancelled() {
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
        if !context.selection().allows(invocation.tool_name.as_str()) {
            return unavailable(&invocation);
        }
        let tool = match self
            .backing
            .registry()
            .get(invocation.tool_name.normalized())
        {
            Some(tool) => tool,
            None => return unavailable(&invocation),
        };
        let context = context.with_authorization(invocation.authorization);

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
        context: &ToolExecutionContext,
    ) -> ToolExecutionOutcome {
        self.execute_checked(invocation, context).await
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
