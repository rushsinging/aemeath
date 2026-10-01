//! 系统提示「技能列表 / Agent 角色」分区的 header / footer 文案。
//!
//! 迁自 runtime `prompt_build_ext.rs` 的 `append_skills` / `append_agent_roles` 内联文案。

pub use super::constants::{
    AGENT_ROLES_FOOTER_EN, AGENT_ROLES_FOOTER_ZH, AGENT_ROLES_HEADER_EN, AGENT_ROLES_HEADER_ZH,
    SKILLS_HEADER_EN, SKILLS_HEADER_ZH,
};

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
#[path = "sections_tests.rs"]
mod tests;
