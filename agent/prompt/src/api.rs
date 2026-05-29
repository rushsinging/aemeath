//! prompt crate 的 Public API 门面（DDD §6.4.3）。
//!
//! 对外仅经此模块暴露 use case 实际消费的 guidance / skill 能力，
//! 内部 guidance / security / skill 模块保持 crate-private。

pub mod guidance {
    pub use crate::business::guidance::resolver::InstructionsLoadedHook;
    pub use crate::business::guidance::{
        init_guidance_dir, resolve_guidance, resolve_guidance_async, UNIVERSAL_EXECUTION_DISCIPLINE,
    };
}

pub mod skill {
    pub use crate::business::skill::{load_all_skills, Skill};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = PromptApiMarker;
        assert_eq!(marker, marker);
    }
}
