//! Universal execution discipline（注入 ALL 模型，不可被 guidance 覆盖）。
//!
//! 迁自 `prompt::business::guidance::constants`。属面向 LLM 注入的核心 system prompt 片段。

pub use super::constants::{UNIVERSAL_EXECUTION_DISCIPLINE_EN, UNIVERSAL_EXECUTION_DISCIPLINE_ZH};

/// Select universal execution discipline by language code (`"en"` / `"zh"`).
/// Falls back to English for unknown languages.
pub fn universal_execution_discipline(lang: &str) -> &'static str {
    match lang {
        "zh" => UNIVERSAL_EXECUTION_DISCIPLINE_ZH,
        _ => UNIVERSAL_EXECUTION_DISCIPLINE_EN,
    }
}

#[cfg(test)]
#[path = "discipline_tests.rs"]
mod tests;
