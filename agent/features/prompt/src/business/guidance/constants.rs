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

<handling_user_followups>
When the user sends a new message while you are mid-task (with or without an active task list),
classify the message BEFORE acting:

1. INTERRUPT — "stop", "wait", "wrong direction", or a hard pivot → Stop the current action immediately.
   Acknowledge, reassess the plan, then continue or adjust.
2. NEW REQUEST — requirement that changes or extends the plan → If urgent, handle it first.
   Otherwise finish the current atomic step, then address it. If a task list is active, update it
   (see task_list_scope_changes).
3. CLARIFICATION — answer to your question or extra detail that does NOT change the plan →
   Integrate the information, continue the current task.
4. ASIDE — quick unrelated question → Answer briefly, then resume the current task.

Priority: INTERRUPT > NEW REQUEST > CLARIFICATION > ASIDE.
When unsure if scope changed, default to CLARIFICATION (do not over-react), but always acknowledge
the latest user message. Never silently ignore a user message — always respond to it before continuing.
</handling_user_followups>

<task_list_scope_changes>
When a task list is active and the user's follow-up message is a NEW REQUEST or INTERRUPT
(see handling_user_followups), you MUST update the active task list and relevant tasks to reflect
the changed plan: modify task descriptions, add tasks, remove tasks, adjust dependencies, or
reprioritize work as needed. If the message is only a CLARIFICATION or ASIDE, keep the current
task list unchanged but still continue with accurate task status.
</task_list_scope_changes>

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

<solution_depth>
When fixing bugs or implementing features, if the same issue can be addressed with both a minimal patch and a thorough root-cause solution, you MUST present both options with their trade-offs, costs, and risks. For recurring, structural, or design-flaw issues, you SHOULD recommend and prioritize the thorough solution unless the user explicitly asks for the minimal patch only.
</solution_depth>
"#;

// ---------------------------------------------------------------------------
// Default guidance files — content is embedded here only for first-run init.
// After init, users edit the md files directly.
// ---------------------------------------------------------------------------

/// Default file names for guidance directory initialization.
/// Files are created empty; users fill in their own content.
pub const DEFAULT_FILE_NAMES: &[&str] = &[
    "_default.md",
    "deepseek.md",
    "glm.md",
    "minimax.md",
    "_reasoning.md",
];

/// Default guidance files for English language (embedded for fallback).
pub const DEFAULT_FILES_EN: &[(&str, &str)] = &[
    (
        "_default.md",
        r#"# Default Guidance
- Think and respond in English unless the user explicitly uses another language.
- Do not proactively generate test cases or documentation (README, etc.) unless explicitly requested.
- Tool call JSON parameters must be strictly valid JSON. Double-check before sending.
- When editing code, always show the exact old_string and new_string — never approximate.
- When using AskUserQuestion with options, the system automatically appends "Type something..." as a built-in option for free-text input. Do NOT include similar options in your options array.
- When using AskUserQuestion with options, prefer object format { "title": "...", "description": "..." } over plain strings. Use description to provide additional context or explanation for each choice.
"#,
    ),
    (
        "deepseek.md",
        r#"# DeepSeek Model Guidance
- Your reasoning/thinking content is displayed separately (thinking mode). Keep it extremely concise: 100 characters or less, 2 sentences max. Do NOT repeat the request, do NOT re-explain code, do NOT include any code snippets in your thinking.
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
        "minimax.md",
        r#"# MiniMax Model Guidance
- Your thinking/reasoning content is displayed separately. In the main response, output conclusions and actions directly.
- Do not repeat your reasoning process in the response body.
"#,
    ),
    (
        "_reasoning.md",
        r#"# Language Preference
- You MUST think/reason in English. Your internal reasoning process must be in English.
- Your final response should also be in English unless the user explicitly writes in another language.
- Keep reasoning concise: output only the final conclusion, no intermediate steps, no code snippets.
"#,
    ),
];

/// Default guidance files for Chinese language.
pub const DEFAULT_FILES_ZH: &[(&str, &str)] = &[
    (
        "_default.md",
        r#"# 默认 Guidance
- 使用中文思考和回复。
- 除非用户明确要求，不要主动生成测试用例、说明文档（README 等）。
- Tool call JSON 参数必须是严格有效的 JSON。发送前请仔细检查。
- 编辑代码时，必须显示精确的 old_string 和 new_string — 不要近似。
- 使用 AskUserQuestion 带选项时，系统会自动添加 "Type something..." 作为自由文本输入选项。不要在 options 数组中包含类似选项。
- 使用 AskUserQuestion 带选项时，优先使用对象格式 { "title": "...", "description": "..." } 而非纯字符串。使用 description 为每个选项提供额外上下文或解释。
"#,
    ),
    (
        "deepseek.md",
        r#"# DeepSeek 模型 Guidance
- 你的推理过程（reasoning_content）必须使用中文。在思考阶段使用中文进行推理和分析。
- 回复内容也使用中文，除非用户明确使用其他语言。
- **强制要求**：reasoning_content 严格限制在 100 字以内。只输出最终结论，禁止中间推导步骤，禁止代码分析，禁止在推理中引用或复制任何代码。超过 2 句话立即停止。这是硬性约束。
"#,
    ),
    (
        "glm.md",
        r#"# GLM 模型 Guidance
- 不要意译或重复工具输出 — 直接引用。
- Tool call JSON 参数必须是严格有效的 JSON。发送前请仔细检查。
- 编辑代码时，必须显示精确的 old_string 和 new_string — 不要近似。
- 你的推理/思考内容会单独显示（thinking mode）。保持极简：100 字以内，最多 2 句话。不要重复请求，不要重新解释代码，不要在思考中包含任何代码片段。
"#,
    ),
    (
        "minimax.md",
        r#"# MiniMax 模型 Guidance
- 你的思考/推理内容会单独显示。在主回复中直接输出结论和行动。
- 不要在回复正文中重复推理过程。
"#,
    ),
    (
        "_reasoning.md",
        r#"# 语言偏好
- 你必须使用中文思考/推理。你的内部推理过程必须是中文。
- 你的最终回复也应使用中文，除非用户明确使用其他语言。
- 保持推理简洁：只输出最终结论，无中间步骤，无代码片段。
"#,
    ),
];

/// All supported languages and their default files.
pub const SUPPORTED_LANGUAGES: &[(&str, &[(&str, &str)])] = &[
    ("en", DEFAULT_FILES_EN),
    ("zh", DEFAULT_FILES_ZH),
];
