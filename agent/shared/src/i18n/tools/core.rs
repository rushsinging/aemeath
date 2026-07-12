//! 核心工具文案（agent/memory/skill/plan_mode/ask_user/brief/sleep/tool_search/lsp 的 description）。

/// Agent description。
pub fn agent(lang: &str) -> &'static str {
    match lang {
        "zh" => "启动一个新代理，自主处理聚焦、限定范围的任务。每个子代理有自己的上下文（约 128K token，默认 200 轮）并可使用所有工具。同一响应中的多个 Agent 调用并发执行。",
        _ => "Launch a new agent to handle a focused, scoped task autonomously.\n\nEach sub-agent has its own context (~128K tokens, default 200 rounds) and can use all tools. Multiple Agent calls in the SAME response run concurrently.",
    }
}

/// Memory description。
pub fn memory(lang: &str) -> &'static str {
    match lang {
        "zh" => "管理持久化记忆。支持 add、delete、search、pin 和 list 操作。",
        _ => "Manage persistent memory. Supports add, delete, search, pin, and list actions.",
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
        "zh" => "向用户提问并等待响应。用 `options` 数组提供预定义选项；永远不要在问题文本中内嵌选项。",
        _ => "Ask the user a question and wait for their response. Use `options` array for predefined choices; never embed choices in the question text.",
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

/// Lsp description。
pub fn lsp(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"使用语言工具获取代码智能信息。

支持的操作：
- diagnostics：获取文件的编译器错误/警告
- definition：查找某位置符号的定义
- references：查找符号的所有引用
- symbols：列出文件或工作区的符号

本工具使用语言特定的 CLI 工具（cargo、tsc、pylint 等）提供代码智能。"#
        }
        _ => {
            r#"Get code intelligence information using language tools.

Supported operations:
- diagnostics: Get compiler errors/warnings for a file
- definition: Find the definition of a symbol at a position
- references: Find all references to a symbol
- symbols: List symbols in a file or workspace

This tool uses language-specific CLI tools (cargo, tsc, pylint, etc.) to provide code intelligence."#
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(brief("zh").contains("简要总结"));
        assert!(sleep("zh").contains("暂停执行"));
        assert!(tool_search("zh").contains("搜索可用工具"));
        assert!(lsp("zh").contains("代码智能"));
    }
}
