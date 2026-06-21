//! Prompt 层文案：系统 prompt、execution discipline、guidance、commit guidance 等。
//!
//! 迁自 `prompt::business::guidance::constants` 与 `runtime::prompt::build` 的面向 LLM 注入文案。

pub mod commit;
pub mod discipline;
pub mod git_context_labels;
pub mod sections;
pub mod system;
