//! Skill Prompt domain.
//!
//! Canonical implementation resides in share（shared kernel）。
//! Prompt domain re-exports from share for interface consistency.

pub use share::skill_ops::*;
pub use share::skill_ops_loader::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_prompt_skill_uses_prompt_skill_parser() {
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
