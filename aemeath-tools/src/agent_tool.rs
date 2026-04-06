use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

pub struct AgentTool;

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Launch a new agent to handle complex, multi-step tasks autonomously.\n\nThe Agent tool launches specialized agents that autonomously handle complex tasks. Each agent has its own conversation context and can use all available tools.\n\nWhen NOT to use the Agent tool:\n- If you want to read a specific file path, use the Read tool instead\n- If you are searching for a specific class definition, use the Glob tool instead\n- If you are searching for code within a specific file, use the Read tool instead\n- Other tasks that are simple and don't require multi-step reasoning\n\nUsage notes:\n- Always include a short description (3-5 words) summarizing what the agent will do\n- Launch multiple agents concurrently whenever possible for independent tasks\n- The agent's outputs should generally be trusted\n- Clearly tell the agent whether you expect it to write code or just to do research\n- Brief the agent like a colleague who just walked into the room -- it hasn't seen this conversation\n- Explain what you're trying to accomplish and why\n- Give enough context that the agent can make judgment calls\n- If you need a short response, say so"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The task for the agent to perform"
                },
                "description": {
                    "type": "string",
                    "description": "A short (3-5 word) description of the task"
                },
                "model": {
                    "type": "string",
                    "enum": ["sonnet", "opus", "haiku"],
                    "description": "Optional model override for this agent"
                }
            },
            "required": ["prompt", "description"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let prompt = match input.get("prompt").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("missing required parameter: prompt"),
        };

        let description = match input.get("description").and_then(|v| v.as_str()) {
            Some(d) => d,
            None => return ToolResult::error("missing required parameter: description"),
        };

        let _model = input
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("sonnet");

        let runner = match &ctx.agent_runner {
            Some(r) => r.clone(),
            None => return ToolResult::error("agent runner not available"),
        };

        eprintln!("[Agent: {description}]");

        let cwd_str = ctx.cwd.to_string_lossy();
        let system = format!(
            r#"You are a sub-agent performing a specific task. You have access to tools for running commands, reading and editing files, and searching codebases.

Working directory: {cwd_str}

Instructions:
- Complete the task described in the user message
- Be thorough but concise
- Report back with your results when done
- If you encounter errors, diagnose and fix them
- Do not ask questions — make reasonable decisions and proceed"#
        );

        let result = runner
            .run_agent(
                prompt,
                &system,
                &[],
                &aemeath_core::tool::ToolRegistry::new(),
                ctx,
            )
            .await;

        ToolResult::success(result)
    }
}
