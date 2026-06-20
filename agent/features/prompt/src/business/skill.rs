//! Skill Prompt domain.
//!
//! Loader（目录遍历 fs IO）与 parser（单文件 fs IO）的 canonical 实现位于本域（refs #61 D2）。
//! `Skill` DTO 仍由共享内核 `share::skill_ops` 承载，此处 re-export 以保持调用方接口一致。

mod loader;
mod parser;

#[cfg(test)]
mod test_support;

pub use loader::{
    load_all_skills, load_all_skills_cached, load_and_filter_skills, load_skills_from_dir,
};
pub use parser::{builtin_commit_skill, parse_skill, read_skill_content};
pub use share::skill_ops::Skill;

#[cfg(test)]
mod tests {
    use super::test_support::unique_skill_dir;
    use super::*;
    use std::io::Write;

    #[test]
    fn test_prompt_skill_uses_prompt_skill_parser() {
        let base = unique_skill_dir("reexport");
        let dir = base.join("cm");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("SKILL.md");
        let mut file = std::fs::File::create(&path).unwrap();
        write!(file, "---\ndescription: test skill\n---\ncontent here").unwrap();

        let skill = parse_skill(&path).unwrap();

        assert_eq!(skill.name, "cm");
        assert_eq!(skill.content, "content here");
        assert_eq!(read_skill_content(&skill), "content here");
        let _ = std::fs::remove_dir_all(&base);
    }
}
