use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
}

/// Parse a skill from a markdown file with YAML frontmatter
/// Format:
/// ```ignore
/// ---
/// name: skill-name
/// description: What this skill does
/// requires_tools:
///   - tool1
///   - tool2
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
    let frontmatter_str = &rest[..end].trim();
    let content = rest[end + 3..].trim().to_string();

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

    let name = if fm.name.is_empty() {
        path.file_stem()?.to_string_lossy().to_string()
    } else {
        fm.name
    };

    Some(Skill {
        name,
        description: fm.description,
        content,
        source_path: path.to_path_buf(),
        requires_tools: fm.requires_tools,
        fallback_for: fm.fallback_for,
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

/// Load skills and filter based on available tools and other skills.
pub fn load_and_filter_skills(
    cwd: &Path,
    available_tools: &std::collections::HashSet<String>,
) -> HashMap<String, Skill> {
    let all_skills = load_all_skills(cwd);
    let skill_names: std::collections::HashSet<String> =
        all_skills.keys().cloned().collect();

    all_skills
        .into_iter()
        .filter(|(_, skill)| {
            // Check requires_tools
            if !skill.requires_tools.is_empty()
                && !skill.requires_tools.iter().all(|t| available_tools.contains(t))
            {
                return false;
            }
            // Check fallback_for
            if skill.fallback_for.iter().any(|s| skill_names.contains(s)) {
                return false;
            }
            true
        })
        .collect()
}

struct SkillsCache {
    skills: HashMap<String, Skill>,
    mtimes: HashMap<PathBuf, std::time::SystemTime>,
}

static SKILLS_CACHE: Mutex<Option<SkillsCache>> = Mutex::new(None);

/// Load skills with caching. Re-scans only if files changed.
pub fn load_all_skills_cached(cwd: &Path) -> HashMap<String, Skill> {
    let mut cache = SKILLS_CACHE.lock().unwrap();

    if let Some(ref cached) = *cache {
        let stale = cached.mtimes.iter().any(|(path, mtime)| {
            std::fs::metadata(path)
                .and_then(|m| m.modified())
                .map(|current| current != *mtime)
                .unwrap_or(true)
        });
        if !stale {
            return cached.skills.clone();
        }
    }

    let skills = load_all_skills(cwd);

    let mtimes: HashMap<PathBuf, std::time::SystemTime> = skills
        .values()
        .filter_map(|s| {
            let mtime = std::fs::metadata(&s.source_path).ok()?.modified().ok()?;
            Some((s.source_path.clone(), mtime))
        })
        .collect();

    *cache = Some(SkillsCache {
        skills: skills.clone(),
        mtimes,
    });

    skills
}
