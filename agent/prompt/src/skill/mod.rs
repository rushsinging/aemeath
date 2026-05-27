//! Skill Prompt domain compatibility facade.
//!
//! Skill 的唯一实现保留在 `share::skill_ops`，避免 Prompt domain 与 share
//! 维护两套解析、加载和内建 skill 逻辑。Prompt 对外继续暴露 `prompt::skill::*`
//! 作为领域语义入口。

pub use share::skill_ops::{
    builtin_commit_skill, load_all_skills, load_all_skills_cached, load_and_filter_skills,
    load_skills_from_dir, parse_skill, read_skill_content, Skill,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_prompt_skill_reexports_share_skill_parser() {
        let base = std::env::temp_dir().join("aemeath_prompt_skill_reexport");
        let dir = base.join("cm");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("SKILL.md");
        let mut file = std::fs::File::create(&path).unwrap();
        write!(file, "---\ndescription: test skill\n---\ncontent here").unwrap();

        let skill = parse_skill(&path).unwrap();

        assert_eq!(skill.name, "cm");
        assert_eq!(read_skill_content(&skill), "content here");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
