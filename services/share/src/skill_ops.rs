//! skill 操作的公共接口
//!
//! tools 通过此模块调用 prompt 的 skill 函数，
//! 避免直接依赖 prompt crate（门禁不允许 tools→prompt）。

pub use prompt::skill::{load_all_skills, read_skill_content, Skill};
