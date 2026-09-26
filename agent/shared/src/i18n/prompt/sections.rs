//! 系统提示「技能列表 / Agent 角色」分区的 header / footer 文案。
//!
//! 迁自 runtime `prompt_build_ext.rs` 的 `append_skills` / `append_agent_roles` 内联文案。

/// 技能列表分区 header（英文）。
pub const SKILLS_HEADER_EN: &str =
    "\n\n# Available Skills\nThe following skills can be invoked with the Skill tool:\n";
/// 技能列表分区 header（中文）。
pub const SKILLS_HEADER_ZH: &str = "\n\n# Available Skills\n以下 skill 可通过 Skill 工具调用：\n";

/// Agent 角色分区 header（英文）。
pub const AGENT_ROLES_HEADER_EN: &str = "\n\n# Available Agent Roles\nThe following agent instances are available for the Agent tool's `agent` parameter. Choose the most appropriate agent for each task:\n";
/// Agent 角色分区 header（中文）。
pub const AGENT_ROLES_HEADER_ZH: &str = "\n\n# Available Agent Roles\n以下 agent 实例可用于 Agent 工具的 `agent` 参数。请为每个任务选择最合适的 agent：\n";

/// Agent 角色分区 footer（英文）。
pub const AGENT_ROLES_FOOTER_EN: &str =
    "\nThe `agent` parameter is required; pick the closest agent from this roster when none fits exactly.";
/// Agent 角色分区 footer（中文）。
pub const AGENT_ROLES_FOOTER_ZH: &str =
    "\n`agent` 参数必填；没有完全合适的 agent 时，从上方名单中选择职能最接近的一个。";

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
        assert!(agent_roles_header("zh").contains("以下 agent 实例"));
        assert!(agent_roles_header("en").contains("The following agent instances"));
        assert_eq!(agent_roles_header("xx"), agent_roles_header("en"));
    }

    #[test]
    fn agent_roles_footer_bilingual_and_fallback_en() {
        assert!(agent_roles_footer("zh").contains("必填"));
        assert!(agent_roles_footer("en").contains("required"));
        assert_eq!(agent_roles_footer("xx"), agent_roles_footer("en"));
    }

    /// roster header/footer 与 `AgentInput::data_schema()` 的 required（`agent`）对齐：
    /// 旧 `role` 口径会诱导 LLM 省略必填 `agent`（issue #1736 R1 复现）。
    #[test]
    fn agent_roster_header_and_footer_declare_agent_field_not_role() {
        for lang in ["zh", "en"] {
            let header = agent_roles_header(lang);
            let footer = agent_roles_footer(lang);
            assert!(
                !header.contains("`role`") && !footer.contains("`role`"),
                "{lang} roster 文案不得出现废弃口径 `role`：header={header} footer={footer}"
            );
            assert!(
                header.contains("`agent`"),
                "{lang} roster header 必须指向 `agent` 字段：{header}"
            );
            assert!(
                !footer.contains("omit") && !footer.contains("省略"),
                "{lang} roster footer 不得诱导省略必填 `agent`：{footer}"
            );
        }
    }
}
