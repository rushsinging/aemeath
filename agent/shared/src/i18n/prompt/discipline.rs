//! Universal execution discipline（注入 ALL 模型，不可被 guidance 覆盖）。
//!
//! 迁自 `prompt::business::guidance::constants`。属面向 LLM 注入的核心 system prompt 片段。

/// Universal execution discipline (English) — injected for ALL models, not overridable.
pub const UNIVERSAL_EXECUTION_DISCIPLINE_EN: &str = r#"# Execution discipline

- Continue until the requested outcome is complete or a concrete blocker requires user input.
- Do not claim completion without verification evidence appropriate to the change.
- When a new user message arrives mid-task, handle interrupts first, incorporate clarifications, and update active task tracking only when scope changes.
- Before acting, verify the relevant repository state, file contents, command prerequisites, and API authentication instead of guessing.
- Prefer a root-cause correction over a symptom workaround; if only a workaround is feasible, state its trade-offs and recurrence risk.
- Keep each delegated or tracked task focused, concrete, and independently verifiable."#;

/// Universal execution discipline (Chinese) — injected for ALL models, not overridable.
pub const UNIVERSAL_EXECUTION_DISCIPLINE_ZH: &str = r#"# 执行纪律

- 持续执行，直到用户要求的结果完成，或遇到必须由用户处理的具体阻断。
- 没有与变更范围匹配的验证证据时，不得声称完成。
- 任务执行中收到新消息时，优先处理中断，整合澄清；仅在范围变化时更新活跃任务追踪。
- 行动前核实相关仓库状态、文件内容、命令前置条件和 API 认证，禁止猜测。
- 优先修复根因而非绕过症状；若只能采用临时方案，必须说明取舍与复发风险。
- 每个委派或追踪任务都应聚焦、具体且可独立验证。"#;

/// Select universal execution discipline by language code (`"en"` / `"zh"`).
/// Falls back to English for unknown languages.
pub fn universal_execution_discipline(lang: &str) -> &'static str {
    match lang {
        "zh" => UNIVERSAL_EXECUTION_DISCIPLINE_ZH,
        _ => UNIVERSAL_EXECUTION_DISCIPLINE_EN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discipline_en_fallback_for_unknown_lang() {
        assert_eq!(
            universal_execution_discipline("fr"),
            UNIVERSAL_EXECUTION_DISCIPLINE_EN
        );
        assert_eq!(
            universal_execution_discipline(""),
            UNIVERSAL_EXECUTION_DISCIPLINE_EN
        );
    }

    #[test]
    fn discipline_zh_selected_for_zh() {
        assert_eq!(
            universal_execution_discipline("zh"),
            UNIVERSAL_EXECUTION_DISCIPLINE_ZH
        );
        assert_ne!(
            UNIVERSAL_EXECUTION_DISCIPLINE_EN,
            UNIVERSAL_EXECUTION_DISCIPLINE_ZH
        );
    }
}
