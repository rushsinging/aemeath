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

        // `timeout` 是框架级通用参数：runtime 已解析成 effective 值写入
        // `ExecutionScope`，工具只读 `ctx.effective_timeout_secs()`。这里在校验前
        // 摘除，使「catalog 注入发布的 schema」与「工具自身 input_schema」对同一份
        // 入参给出一致判定，避免把合法 `timeout` 判成未知字段。
        let mut input = invocation.input;
        crate::domain::timeout_policy::take_timeout_from_input(&mut input);

        if let Err(mismatch) = crate::domain::schema_validator::validate_tool_input(
            tool.name(),
            &tool.input_schema(),
            &input,
        ) {
            return ToolExecutionOutcome::failure(
                ToolErrorKind::InvalidInput,
                crate::domain::schema_validator::format_tool_input_error(&mismatch),
            );
        }

        if let Some(suspension) = tool.suspension(&input) {
            return match suspension {
                Ok(value) => ToolExecutionOutcome::Suspended(value),
                Err(message) => ToolExecutionOutcome::failure(ToolErrorKind::InvalidInput, message),
            };
        }

        map_legacy_result(tool.call(input, &context).await)
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

    fn goal_text(&self, call: &crate::domain::published_language::ToolCall) -> String {
        match self.backing.registry().get(call.tool_name.as_str()) {
            Some(tool) => tool.goal_text(&call.input),
            None => call.goal_text(),
        }
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
            task_change: result.task_change,
        })
    }
}

// #2146 密封解锁。
impl crate::sealed::Sealed for ExecutionAdapter {}
