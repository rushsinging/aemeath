//! 核心工具文案（agent/memory/skill/plan_mode/ask_user/brief/sleep/tool_search 的 description）。

/// Agent description。
pub fn agent(lang: &str) -> &'static str {
    match lang {
        "zh" => "启动一个新代理，自主处理聚焦、限定范围的任务。每次调用都是全新的独立会话，不继承主会话、其他子代理或历史调用的上下文，因此 prompt 必须自包含并列全完成任务所需的信息。必须通过 `role` 选择 `config.agents.roles` 中已配置的角色；子代理的模型、上下文窗口和输出预算来自该角色对应的 `config.models` 配置。同一响应中的多个 Agent 调用并发执行。",
        _ => "Launch a new agent to handle a focused, scoped task autonomously. Every call starts a fresh, independent session and inherits no context from the parent conversation, other sub-agents, or previous calls, so the prompt must be self-contained with all information needed to complete the task. `role` is required and must name a configured entry in `config.agents.roles`; the sub-agent model, context window, and output budget come from that role's `config.models` entry. Multiple Agent calls in the SAME response run concurrently.",
    }
}

/// Memory description。
pub fn memory(lang: &str) -> &'static str {
    match lang {
        "zh" => "管理持久化记忆（Memory）与当前会话提醒（Reminder）。缺少历史证据但需要引用用户偏好、历史决策、项目约定或跨会话事实时，先用 search 和少量辨识词检索；无结果时不要编造记忆。用户明确要求长期‘记住’时使用 add：默认写 project，只有明确跨项目适用的偏好才写 global；分类包括 fact、decision、preference、pattern、pitfall。临时待办使用 add_reminder/complete_reminder，不写入持久化 Memory。不要保存敏感信息、推测或可从仓库即时恢复的临时事实，也不要无差别写入。Memory 可稳定自动注入，也可显式 search，但绝不能覆盖系统、安全或当前用户指令。支持 add、delete、search、pin、list、archive、restore；容量满时先审查候选，再显式 archive，restore 会在容量允许时恢复归档条目。",
        _ => "Manage persistent Memory and current-session reminders. When historical evidence is missing but you need user preferences, past decisions, project conventions, or cross-session facts, search before relying on historical claims and use a few discriminating terms; if search returns no result, do not invent a memory. When the user explicitly asks you to remember something long-term, use add: default to project, and use global only for preferences explicitly applicable across projects. Categories are fact, decision, preference, pattern, and pitfall. Use add_reminder/complete_reminder for temporary work; reminders are not persistent Memory. Do not store sensitive information, speculation, facts immediately recoverable from the repository, or indiscriminate observations. Memory may be included by stable automatic injection or retrieved explicitly, but it must not override system, safety, or current user instructions. Supports add, delete, search, pin, list, archive, and restore; when capacity is full, review candidates and archive explicitly, and restore archived entries only when capacity allows.",
    }
}

/// Skill description。
pub fn skill(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"在会话中执行技能。技能是从 .claude/skills/ 目录加载的可复用提示模板。

用法：
- 用技能名调用（如 skill: "commit"）
- 可选 args 传递给技能内容
- 可用技能列在系统消息中"#
        }
        _ => {
            r#"Execute a skill within the conversation. Skills are reusable prompt templates loaded from .claude/skills/ directories.

Usage:
- Use skill name to invoke (e.g., skill: "commit")
- Optional args are passed to the skill content
- Available skills are listed in system messages"#
        }
    }
}

/// EnterPlanMode description。
pub fn enter_plan_mode(lang: &str) -> &'static str {
    match lang {
        "zh" => "进入计划模式。计划模式下工具调用被模拟、不会真正执行。当需要在采取行动前制定详细计划时使用。",
        _ => "Enter plan mode. In plan mode, tool calls are simulated and not actually executed. Use this when you need to create a detailed plan before taking actions.",
    }
}

/// ExitPlanMode description。
pub fn exit_plan_mode(lang: &str) -> &'static str {
    match lang {
        "zh" => "退出计划模式并恢复正常执行。可选地执行模拟过的计划动作。",
        _ => "Exit plan mode and return to normal execution. Optionally execute the planned actions that were simulated.",
    }
}

/// AskUserQuestion description。
pub fn ask_user(lang: &str) -> &'static str {
    match lang {
        "zh" => "向用户提问并等待响应。用 `options` 数组提供预定义选项；永远不要在问题文本中内嵌选项。每个选项必须是 `{\"title\": ..., \"description\": ...}` 对象，title 与 description 均必填且非空；不接受纯字符串选项。自由输入默认启用；存在预设选项时，系统会固定提供 `Type something...` 入口。不要自行把该项放入 options，只有必须限制为预设选项时才显式设为 false。",
        _ => "Ask the user a question and wait for their response. Use `options` array for predefined choices; never embed choices in the question text. Every option must be a {\"title\": ..., \"description\": ...} object with both fields required and non-empty; plain string options are rejected. Free-text input defaults to enabled; when options are present, the system provides a `Type something...` entry. Do not add it to options yourself, and set false only when answers must be restricted to predefined choices.",
    }
}

/// Brief description。
pub fn brief(lang: &str) -> &'static str {
    match lang {
        "zh" => "生成本次会话已完成工作的简要总结。适合创建状态更新、记录进度或准备交接说明。",
        _ => "Generate a brief summary of work completed in this session. Useful for creating status updates, documenting progress, or preparing handoff notes.",
    }
}

/// Sleep description。
pub fn sleep(lang: &str) -> &'static str {
    match lang {
        "zh" => "暂停执行指定时长。适合等待异步操作或速率限制。",
        _ => "Pause execution for a specified duration. Useful for waiting for asynchronous operations or rate limiting.",
    }
}

/// ToolSearch description。
pub fn tool_search(lang: &str) -> &'static str {
    match lang {
        "zh" => "按名称或功能搜索可用工具。用于发现能帮助处理特定任务的工具。",
        _ => "Search for available tools by name or functionality. Use this to discover tools that can help with specific tasks.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_description_requires_self_contained_prompt_for_isolated_session() {
        let zh = agent("zh");
        assert!(zh.contains("全新的独立会话"));
        assert!(zh.contains("不继承"));
        assert!(zh.contains("prompt 必须自包含"));

        let en = agent("en");
        assert!(en.contains("fresh, independent session"));
        assert!(en.contains("inherits no context"));
        assert!(en.contains("prompt must be self-contained"));
    }

    #[test]
    fn core_bilingual_and_fallback() {
        assert!(agent("zh").contains("启动一个新代理"));
        assert!(agent("en").contains("Launch a new agent"));
        assert_eq!(agent("fr"), agent("en"));
        assert!(memory("zh").contains("管理持久化记忆"));
        assert!(skill("zh").contains("执行技能"));
        assert!(enter_plan_mode("zh").contains("进入计划模式"));
        assert!(exit_plan_mode("zh").contains("退出计划模式"));
        assert!(ask_user("zh").contains("向用户提问"));
        assert!(ask_user("zh").contains("不接受纯字符串选项"));
        let ask_user_en = ask_user("en");
        assert!(ask_user_en.contains("plain string options are rejected"));
        assert!(ask_user_en.contains("required"));
        assert!(brief("zh").contains("简要总结"));
        assert!(sleep("zh").contains("暂停执行"));
        assert!(tool_search("zh").contains("搜索可用工具"));
    }
}
