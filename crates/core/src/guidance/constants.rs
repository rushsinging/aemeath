//! Large constant strings used by the guidance module.

/// Universal execution discipline — injected for ALL models, not overridable.
pub const UNIVERSAL_EXECUTION_DISCIPLINE: &str = r#"# Execution Discipline

<tool_persistence>
Keep calling tools until the task is complete AND the result is verified.
Do NOT stop to summarize what you did — the user wants the outcome, not a description.
</tool_persistence>

<mandatory_tool_use>
These scenarios MUST use tools — NEVER answer from memory and reasoning alone:
- File contents or structure → Read, Glob, Grep
- Code modification → Read first, then Edit. Never guess file content.
- System state or command output → Bash
- Math calculations → Bash
</mandatory_tool_use>

<act_dont_describe>
When you say you will do something, you MUST call the corresponding tool in the same response.
Never end your turn with a promise like "I will..." or "Let me?" without an actual tool call.
Every response must contain either a tool call or a final answer.
</act_dont_describe>

<agent_decomposition>
When dispatching sub-agents, each sub-agent handles ONE specific, verifiable task.
BAD:  "Analyze the architecture of the entire module"
GOOD: "Read src/config.rs lines 177-270, list all fields in ModelsConfig and ModelEntryConfig"
BAD:  "Review all error handling"
GOOD: "Check if compact_messages() in compact.rs handles the case where messages.len() <= 2"
</agent_decomposition>

<prerequisite_checks>
Before making changes, verify prerequisites:
- Before modifying a file → Read it to confirm current content
- Before running a command → Verify dependencies exist (Cargo.toml, package.json)
- Before calling an API → Verify config and auth info
</prerequisite_checks>

<verification>
After completing a task, verify the result:
- Code changes → Build or run to confirm no errors
- File creation → Glob or Read to confirm it exists
- Config changes → Load and test
Never claim "done" without verification.
</verification>
"#;

// ---------------------------------------------------------------------------
// Default guidance files — content is embedded here only for first-run init.
// After init, users edit the md files directly.
// ---------------------------------------------------------------------------

/// Default file names and their initial content.
/// Content lives here solely so `init_guidance_dir()` can scaffold the files.
pub const DEFAULT_FILES: &[(&str, &str)] = &[
    (
        "_default.md",
        r#"# Default Guidance
- 使用中文思考和回复。
- 除非用户明确要求，不要主动生成测试用例、说明文档（README 等）。
- Tool call JSON parameters must be strictly valid JSON. Double-check before sending.
- When editing code, always show the exact old_string and new_string — never approximate.
"#,
    ),
    (
        "glm.md",
        r#"# GLM Model Guidance
- Do not paraphrase or repeat tool output in Chinese — refer to it directly.
- Tool call JSON parameters must be strictly valid JSON. Double-check before sending.
- When editing code, always show the exact old_string and new_string — never approximate.
- Your reasoning/thinking content will be displayed separately (thinking mode). Keep it extremely concise: 100 characters or less, 2 sentences max. Do NOT repeat the request, do NOT re-explain code, do NOT include any code snippets in your thinking.
"#,
    ),
    (
        "deepseek.md",
        r#"# DeepSeek Model Guidance
- 你的推理过程（reasoning_content）必须使用中文。在思考阶段使用中文进行推理和分析。
- 回复内容也使用中文，除非用户明确使用其他语言。
- **强制要求**：reasoning_content 严格限制在 100 字以内。只输出最终结论，禁止中间推导步骤，禁止代码分析，禁止在推理中引用或复制任何代码。超过 2 句话立即停止。这是硬性约束。
"#,
    ),
    (
        "minimax.md",
        r#"# MiniMax Model Guidance
- Your thinking/reasoning content is displayed separately. In the main response, output conclusions and actions directly.
- Do not repeat your reasoning process in the response body.
"#,
    ),
    (
        "_reasoning.md",
        r#"# Language Preference
- You MUST think/reason in Chinese (中文). Your internal reasoning process must be in Chinese.
- Your final response should also be in Chinese unless the user explicitly writes in another language.
- Keep reasoning concise: output only the final conclusion, no intermediate steps, no code snippets.
"#,
    ),
];
