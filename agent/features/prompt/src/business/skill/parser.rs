use crate::LOG_TARGET;

use serde::Deserialize;
use share::skill_ops::Skill;
use std::path::{Path, PathBuf};

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

const BUILTIN_SKILL_PREFIX: &str = "aemeath-builtin://";

pub fn builtin_commit_skill() -> Skill {
    Skill {
        name: "commit".to_string(),
        description: "Create a git commit using the repository's Commit Style Context".to_string(),
        content: String::new(),
        source_path: PathBuf::from(format!("{BUILTIN_SKILL_PREFIX}commit")),
        requires_tools: Vec::new(),
        fallback_for: Vec::new(),
        aliases: vec!["git-commit".to_string()],
    }
}

/// Parse a skill from a markdown file with YAML frontmatter.
///
/// Reads the YAML frontmatter (name, description, aliases, etc.) and materializes
/// the markdown body into `Skill::content`, so downstream Tool execution does not
/// need filesystem access.
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
            log::warn!(target: LOG_TARGET,
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
        content: read_skill_content_from_path(path),
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
    if skill.source_path.to_string_lossy() == format!("{BUILTIN_SKILL_PREFIX}commit") {
        return builtin_commit_skill_content().to_string();
    }
    read_skill_content_from_path(&skill.source_path)
}

fn read_skill_content_from_path(path: &Path) -> String {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            log::warn!(target: LOG_TARGET, "failed to read skill content from {}: {e}", path.display());
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

fn builtin_commit_skill_content() -> &'static str {
    r#"# Built-in commit skill

Use this skill whenever you need to create a git commit.

## Required workflow

1. Inspect the working tree with `git status --short --branch`.
2. Inspect repository commit style before writing a message. Prefer commits with AI co-author trailers:
   `git log --format=%B --grep='Co-Authored-By' -n 20`
3. If there are no useful co-author examples, sample recent ordinary commits with a small limit.
4. Inspect staged and unstaged changes enough to understand the commit scope.
5. Generate a commit message that matches this repository's Commit Style Context: title format, type/scope usage, body language, body style, footer/trailer conventions, and whether AI co-author trailers are commonly used.
6. Do not invent human co-authors.
7. When an AI co-author trailer is appropriate, use the exact `Co-Authored-By: Aemeath (...) <github:rushsinging/aemeath>` trailer supplied by the current system prompt.
8. Run `git commit` with the generated message.

## Safety rules

- Do not stage unrelated files unless the user explicitly asks.
- Do not amend unless the user explicitly asks.
- If the working tree contains unrelated user changes, report them and commit only the intended paths.
- If verification is required by the active task, verify before committing.
"#
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
        assert_eq!(skill.content, "content here");

        // 清理是 best-effort：macOS/APFS 在并发编译或同步层干扰下偶发
        // ENOTEMPTY（code 66），清理失败不代表被测逻辑有误。
        let _ = std::fs::remove_dir_all(&base);
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
        assert_eq!(skill.content, "content");

        let _ = std::fs::remove_dir_all(&base);
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
        assert_eq!(skill.content, "Full body content here!");

        let content = read_skill_content(&skill);
        assert_eq!(content, "Full body content here!");

        let _ = std::fs::remove_dir_all(&base);
    }
}
