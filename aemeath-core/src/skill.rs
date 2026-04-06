use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A skill definition loaded from a markdown file with YAML frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub content: String,
    pub source_path: PathBuf,
}

/// Parse a skill from a markdown file with YAML frontmatter
/// Format:
/// ```ignore
/// ---
/// name: skill-name
/// description: What this skill does
/// ---
/// Content here...
/// ```
pub fn parse_skill(path: &Path) -> Option<Skill> {
    let text = std::fs::read_to_string(path).ok()?;

    if !text.starts_with("---") {
        return None;
    }

    let rest = &text[3..];
    let end = rest.find("---")?;
    let frontmatter = &rest[..end].trim();
    let content = rest[end + 3..].trim().to_string();

    // Simple YAML parsing for name and description
    let mut name = String::new();
    let mut description = String::new();

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().trim_matches('"').trim_matches('\'').to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').trim_matches('\'').to_string();
        }
    }

    if name.is_empty() {
        // Use filename as name
        name = path.file_stem()?.to_string_lossy().to_string();
    }

    Some(Skill {
        name,
        description,
        content,
        source_path: path.to_path_buf(),
    })
}

/// Load all skills from a directory (non-recursive)
pub fn load_skills_from_dir(dir: &Path) -> Vec<Skill> {
    if !dir.exists() {
        return Vec::new();
    }

    let mut skills = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                if let Some(skill) = parse_skill(&path) {
                    skills.push(skill);
                }
            }
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Load skills from all standard locations
pub fn load_all_skills(cwd: &Path) -> HashMap<String, Skill> {
    let mut map = HashMap::new();

    // Project-level skills: .claude/skills/
    let project_dir = cwd.join(".claude").join("skills");
    for skill in load_skills_from_dir(&project_dir) {
        map.insert(skill.name.clone(), skill);
    }

    // Global skills: ~/.claude/skills/
    if let Some(home) = dirs::home_dir() {
        let global_dir = home.join(".claude").join("skills");
        for skill in load_skills_from_dir(&global_dir) {
            map.entry(skill.name.clone()).or_insert(skill);
        }
    }

    map
}
