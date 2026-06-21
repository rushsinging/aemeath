//! 系统提示「技能列表 / Agent 角色」分区的 header / footer 文案。
//!
//! 迁自 runtime `prompt_build_ext.rs` 的 `append_skills` / `append_agent_roles` 内联文案。

/// 技能列表分区 header（英文）。
pub const SKILLS_HEADER_EN: &str =
    "\n\n# Available Skills\nThe following skills can be invoked with the Skill tool:\n";
/// 技能列表分区 header（中文）。
pub const SKILLS_HEADER_ZH: &str = "\n\n# Available Skills\n以下 skill 可通过 Skill 工具调用：\n";

/// Agent 角色分区 header（英文）。
pub const AGENT_ROLES_HEADER_EN: &str = "\n\n# Available Agent Roles\nThe following agent roles are available for the Agent tool's `role` parameter. Choose the most appropriate role for each task:\n";
/// Agent 角色分区 header（中文）。
pub const AGENT_ROLES_HEADER_ZH: &str = "\n\n# Available Agent Roles\n以下 agent role 可用于 Agent 工具的 `role` 参数。请为每个任务选择最合适的 role：\n";

/// Agent 角色分区 footer（英文）。
pub const AGENT_ROLES_FOOTER_EN: &str =
    "\nWhen no role fits, omit the `role` parameter to use the default model.";
/// Agent 角色分区 footer（中文）。
pub const AGENT_ROLES_FOOTER_ZH: &str = "\n没有合适的 role 时，省略 `role` 参数以使用默认模型。";

/// 按语言选择技能列表 header。未知 lang 回退英文。
pub fn skills_header(lang: &str) -> &'static str {
    match lang {
        "zh" => SKILLS_HEADER_ZH,
        _ => SKILLS_HEADER_EN,
    }
}

/// 按语言选择 Agent 角色 header。未知 lang 回退英文。
pub fn agent_roles_header(lang: &str) -> &'static str {
    match lang {
        "zh" => AGENT_ROLES_HEADER_ZH,
        _ => AGENT_ROLES_HEADER_EN,
    }
}

/// 按语言选择 Agent 角色 footer。未知 lang 回退英文。
pub fn agent_roles_footer(lang: &str) -> &'static str {
    match lang {
        "zh" => AGENT_ROLES_FOOTER_ZH,
        _ => AGENT_ROLES_FOOTER_EN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_header_bilingual_and_fallback_en() {
        assert!(skills_header("zh").contains("以下 skill"));
        assert!(skills_header("en").contains("The following skills"));
        assert_eq!(skills_header("fr"), skills_header("en"));
    }

    #[test]
    fn agent_roles_header_bilingual_and_fallback_en() {
        assert!(agent_roles_header("zh").contains("以下 agent role"));
        assert!(agent_roles_header("en").contains("The following agent roles"));
        assert_eq!(agent_roles_header("xx"), agent_roles_header("en"));
    }

    #[test]
    fn agent_roles_footer_bilingual_and_fallback_en() {
        assert!(agent_roles_footer("zh").contains("省略"));
        assert!(agent_roles_footer("en").contains("omit"));
        assert_eq!(agent_roles_footer("xx"), agent_roles_footer("en"));
    }
}
