//! 核心工具文案（agent/memory/skill/plan_mode/ask_user/brief/sleep/tool_search 的 description）。

/// Agent description。
///
/// Agent 工具描述的**唯一真相源**：`AgentTool::description()` / `description_for()`
/// 与注入 LLM 的 tool schema 均由此分派，NEVER 再维护第二份口径文本。
pub fn agent(lang: &str) -> &'static str {
    match lang {
        "zh" => "启动一个新代理处理聚焦任务。每次调用都是全新的独立会话，不继承上下文，因此 prompt 必须自包含。`agent` 为必填，必须匹配 `config.agents.names` 中的名称。",
        _ => "Launch a new agent for focused tasks. Each call is a fresh, independent session that inherits no context; the prompt must be self-contained. `agent` is required; match a name in `config.agents.names`.",
    }
}

/// MemoryAdd description。
pub fn memory_add(lang: &str) -> &'static str {
    match lang {
        "zh" => "写入一条持久记忆，用于记录用户明确要求长期保留的偏好、决策、项目约定与跨会话事实。内容相近会自动合并，不会新建重复条目。",
        _ => "Write one persistent memory for a preference, decision, project convention, or cross-session fact the user asked to keep. Similar content is merged instead of duplicated.",
    }
}

/// MemorySearch description。
pub fn memory_search(lang: &str) -> &'static str {
    match lang {
        "zh" => "按关键词检索已有持久记忆。缺少历史证据时先用它查找，不要凭猜测断言。",
        _ => "Search existing persistent memory by keywords. Use it before asserting historical facts instead of guessing.",
    }
}

/// MemoryList description。
pub fn memory_list(lang: &str) -> &'static str {
    match lang {
        "zh" => "列出持久记忆条目，用于审阅当前有哪些记忆、哪些已归档。",
        _ => "List persistent memory entries to review what exists and what is archived.",
    }
}

/// MemoryUpdate description。
pub fn memory_update(lang: &str) -> &'static str {
    match lang {
        "zh" => "在 pin、unpin、archive、restore 之间变更记忆状态。容量满时先审查候选项再归档。",
        _ => "Change a memory's status among pin, unpin, archive, and restore. When capacity is full, review candidates before archiving.",
    }
}

/// MemoryDelete description。
pub fn memory_delete(lang: &str) -> &'static str {
    match lang {
        "zh" => "永久删除一条记忆。仅在用户明确要求删除时使用。",
        _ => "Permanently delete a memory. Use only when the user explicitly asks to delete it.",
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
        "zh" => "向用户提问并等待回答。当需要用户输入或确认才能继续时使用；选项格式以字段 schema 为准。",
        _ => "Ask the user one or more questions and wait for their response. Use this when input or confirmation from the user is required to proceed. Option format is defined in the field schema.",
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
#[path = "core_tests.rs"]
mod tests;
