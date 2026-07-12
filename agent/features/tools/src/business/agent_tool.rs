use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use crate::LOG_TARGET;
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::agent::{AgentInput, AgentResult};

const SUB_AGENT_MAX_TURNS_CAP: u32 = 1000;

pub struct AgentTool;

#[async_trait]
impl TypedTool for AgentTool {
    type Output = AgentResult;
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Launch a new agent to handle a focused, scoped task autonomously.\n\nEach sub-agent has its own context (~128K tokens, up to 1000 rounds) and can use all tools. Multiple Agent calls in the SAME response run concurrently."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::agent(lang))
    }

    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        AgentInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        AgentResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true // Sub-agents are independent — safe to run in parallel
    }

    fn timeout_secs(&self) -> u64 {
        1800 // 30 minutes — sub-agents run multi-turn LLM conversations
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<AgentResult> {
        let args: AgentInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };

        if args.prompt.is_empty() {
            return TypedToolResult::error("missing required parameter: prompt");
        }
        if args.description.is_empty() {
            return TypedToolResult::error("missing required parameter: description");
        }
        let prompt = args.prompt.as_str();

        let cwd = ctx.workspace_read().current_path_base();
        let max_turns = args
            .max_turns
            .map(|v| (v as u32).min(SUB_AGENT_MAX_TURNS_CAP))
            .unwrap_or(SUB_AGENT_MAX_TURNS_CAP);
        let runner = match &ctx.resources.agent_runner {
            Some(r) => r.clone(),
            None => return TypedToolResult::error("agent runner not available"),
        };

        let cwd_str = cwd.to_string_lossy();
        let turns = max_turns;

        // `role` is resolved by CliAgentRunner via AgentsConfig::roles.
        // If not set, the runner uses the default model.
        let model_spec = args.role.as_deref();

        let system = format!(
            r#"You are a sub-agent performing a specific task. You have access to tools for running commands, reading and editing files, and searching codebases.

Working directory: {cwd_str}

CRITICAL — Context budget:
- You have a LIMITED context window (~128K tokens). Every file you read consumes tokens.
- ALWAYS use `limit` parameter when reading files: `Read(file_path, limit: 100)` for overviews, `limit: 50` for quick checks. Only omit limit for very small files (<100 lines).
- Use Grep to find specific code instead of reading entire files — this is much more token-efficient.
- Use Glob to discover files, then read only the most relevant ones.
- NEVER read more than 3 files per tool call round.
- You have max {turns} rounds of tool calls. Plan ahead — don't waste rounds reading files you won't use.
- You cannot use Task*, AskUserQuestion, or Agent tools. Task tracking and user clarification belong to the parent agent.
- If the task is ambiguous or needs user input, return a concise `blocked:` explanation instead of asking the user.

Instructions:- Complete the task described in the user message
- Be thorough but concise
- Once you have enough information, STOP using tools and write your final response immediately
- Do not ask questions — make reasonable decisions and proceed"#
        );

        let final_output = runner
            .run_agent(crate::api::AgentRunRequest {
                prompt,
                system: &system,
                ctx,
                max_turns,
                model_spec,
                progress_tx: ctx.progress_tx.clone(),
            })
            .await;

        // text → 父 LLM：必须包含子代理实际产出，否则父 agent 无法基于结果决策。
        // data → TUI：结构化展示（AgentResult.output 同步保留）。
        let text = if final_output.trim().is_empty() {
            "子代理执行完成（无输出）".to_string()
        } else {
            final_output.clone()
        };
        log::debug!(
            target: LOG_TARGET,
            "agent final output: output_bytes={}, text_bytes={}, output_preview={:?}",
            final_output.len(),
            text.len(),
            truncate_debug_preview(&final_output, 500)
        );
        TypedToolResult::success(
            text,
            AgentResult {
                output: final_output,
            },
        )
    }
}

fn truncate_debug_preview(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

#[cfg(test)]
#[path = "agent_tool_tests.rs"]
mod agent_tool_tests;
