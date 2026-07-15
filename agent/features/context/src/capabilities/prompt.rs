//! Prompt & Guidance 子模块（PromptPort）。
//!
//! 原 `agent/features/prompt/` crate 整体并入。

#[allow(dead_code, unused_imports)]
pub(crate) mod guidance;
pub(crate) mod security;
#[allow(dead_code, unused_imports)]
pub(crate) mod skill;

pub use guidance::resolver::InstructionsLoadedHook;
pub use guidance::{
    init_guidance_dir, resolve_guidance, resolve_guidance_async, universal_execution_discipline,
};
pub use skill::{
    builtin_commit_skill, load_all_skills, load_all_skills_cached, load_and_filter_skills,
    load_skills_from_dir, parse_skill, read_skill_content, Skill,
};

/// 旧 crate 的日志 target，prompt 内部 log 调用仍使用。
pub const LOG_TARGET: &str = "aemeath:agent:prompt";
