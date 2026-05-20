pub mod loader;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// Re-export loader functions so external callers don't need to change paths.
pub use loader::{
    load_all_skills, load_all_skills_cached, load_and_filter_skills, load_skills_from_dir,
};

/// A skill definition loaded from a markdown file with YAML frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub content: String,
    pub source_path: PathBuf,
    /// Tools required for this skill to be available
    #[serde(default)]
    pub requires_tools: Vec<String>,
    /// If these skills are available, hide this one (it's a fallback)
    #[serde(default)]
    pub fallback_for: Vec<String>,
    /// Slash command aliases (e.g. ["cm"] means /cm invokes this skill)
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Intermediate struct for deserializing YAML frontmatter
#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    requires_tools: Vec<String>,
    #[serde(default)]
    fallback_for: Vec<String>,
    #[serde(default)]
    aliases: Vec<String>,
}

/// Parse a skill from a markdown file with YAML frontmatter.
///
/// Only reads the YAML frontmatter (name, description, aliases, etc.) and
/// records the file path. The body content is **not** read at this point —
/// it is loaded lazily when the Skill tool is invoked (see
/// [`read_skill_content`]).
///
/// If the frontmatter does not specify a `name`, the stem of the **parent
/// directory** is used (so `cm/SKILL.md` gets name `cm`).  When the inferred
/// name differs from the file stem, the directory name is also automatically
/// added as an alias so that `/cm` resolves to this skill.
pub fn parse_skill(path: &Path) -> Option<Skill> {
    let text = std::fs::read_to_string(path).ok()?;

    if !text.starts_with("---") {
        return None;
    }

    let rest = &text[3..];
    let end = rest.find("---")?;
    let frontmatter_str = &rest[..end].trim();

    // Parse YAML using serde_yml — handles standard YAML lists, multi-line values, etc.
    let fm: SkillFrontmatter = match serde_yml::from_str(frontmatter_str) {
        Ok(fm) => fm,
        Err(e) => {
            log::warn!(
                "failed to parse YAML frontmatter in {}: {e}",
                path.display()
            );
            return None;
        }
    };

    // Name resolution priority: frontmatter name > file stem
    // Special case: when the file is named "SKILL.md" (case-insensitive),
    // use the parent directory name instead (e.g. cm/SKILL.md → name "cm").
    let dir_name = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());
    let file_stem = path.file_stem()?.to_string_lossy().to_string();
    let is_generic_name = file_stem.eq_ignore_ascii_case("skill")
        || file_stem.eq_ignore_ascii_case("index")
        || file_stem.eq_ignore_ascii_case("README");

    let name = if !fm.name.is_empty() {
        fm.name
    } else if is_generic_name {
        // Generic filename → use parent directory name
        dir_name.clone().unwrap_or(file_stem.clone())
    } else {
        file_stem.clone()
    };

    // Auto-add directory name as alias when the skill lives in a sub-directory
    // AND the directory name is not already the skill name or an existing alias.
    let mut aliases = fm.aliases;
    if let Some(ref dir) = dir_name {
        if dir.as_str() != name && !aliases.contains(dir) {
            aliases.push(dir.clone());
        }
    }

    Some(Skill {
        name,
        description: fm.description,
        content: String::new(), // lazy-loaded by read_skill_content()
        source_path: path.to_path_buf(),
        requires_tools: fm.requires_tools,
        fallback_for: fm.fallback_for,
        aliases,
    })
}

/// Read the full body content of a skill from its source file.
///
/// Returns the markdown body (everything after the closing `---` of the
/// YAML frontmatter). If the file cannot be read, returns an empty string.
pub fn read_skill_content(skill: &Skill) -> String {
    let text = match std::fs::read_to_string(&skill.source_path) {
        Ok(t) => t,
        Err(e) => {
            log::warn!(
                "failed to read skill content from {}: {e}",
                skill.source_path.display()
            );
            return String::new();
        }
    };

    if !text.starts_with("---") {
        return text;
    }

    let rest = &text[3..];
    if let Some(end) = rest.find("---") {
        rest[end + 3..].trim().to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_skill_dir_name_as_name() {
        // Simulate real layout: <skills-dir>/cm/SKILL.md
        let base = std::env::temp_dir().join("aemeath_test_skill_1");
        let dir = base.join("cm");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("SKILL.md");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "---\ndescription: test skill\n---\ncontent here").unwrap();

        let skill = parse_skill(&path).unwrap();
        assert_eq!(skill.name, "cm", "expected name from parent dir");
        assert!(skill.aliases.is_empty());
        assert!(
            skill.content.is_empty(),
            "content should not be loaded at scan time"
        );

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_parse_skill_alias_from_dir() {
        // frontmatter specifies a different name than the dir name
        let base = std::env::temp_dir().join("aemeath_test_skill_2");
        let dir = base.join("my-dir");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("SKILL.md");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "---\nname: my-skill\ndescription: test\n---\ncontent").unwrap();

        let skill = parse_skill(&path).unwrap();
        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.aliases, vec!["my-dir"]);
        assert!(skill.content.is_empty());

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_read_skill_content_lazy() {
        let base = std::env::temp_dir().join("aemeath_test_skill_lazy");
        let dir = base.join("my-skill");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("SKILL.md");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            "---\nname: my-skill\ndescription: test\n---\nFull body content here!"
        )
        .unwrap();

        let skill = parse_skill(&path).unwrap();
        assert!(skill.content.is_empty(), "scan should not load content");

        let content = read_skill_content(&skill);
        assert_eq!(content, "Full body content here!");

        std::fs::remove_dir_all(&base).unwrap();
    }
}
