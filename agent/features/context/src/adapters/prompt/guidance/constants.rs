//! Large constant strings used by the guidance module.
//!
//! 注：`UNIVERSAL_EXECUTION_DISCIPLINE_EN/ZH` 及 `universal_execution_discipline()` 已迁至
//! `share::i18n::prompt::discipline`（项目级 i18n catalog 单一真相）。
//! 本文件仅保留 guidance 目录首次初始化用的默认文件数据（非注入 LLM 文案）。

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
- AskUserQuestion enables free-text input by default. When predefined options are present, the system provides "Type something..." as the built-in free-text entry; do NOT include a similar option yourself. Set `allow_free_input: false` only when answers must be limited to the supplied options.
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
- AskUserQuestion 默认启用自由输入。存在预设选项时，系统会固定提供 "Type something..." 作为内建自由输入入口；不要自行在 options 中包含类似选项。只有答案必须限制为所给选项时，才设置 `allow_free_input: false`。
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
pub const SUPPORTED_LANGUAGES: &[(&str, &[(&str, &str)])] =
    &[("en", DEFAULT_FILES_EN), ("zh", DEFAULT_FILES_ZH)];
