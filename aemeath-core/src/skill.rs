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
/// Parse a skill from a markdown file with YAML frontmatter.
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

    // Name resolution priority: frontmatter name > file stem
    // Special case: when the file is named "SKILL.md" (case-insensitive),
    // use the parent directory name instead (e.g. cm/SKILL.md → name "cm").
    let dir_name = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
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
        content,
        source_path: path.to_path_buf(),
        requires_tools: fm.requires_tools,
        fallback_for: fm.fallback_for,
        aliases,
    })
}

/// Load all skills from a directory.
///
/// Scans `.md` files directly in the directory **and** recursively inside
/// immediate sub-directories (supports the `<skill-name>/SKILL.md` layout
/// used by Claude Code / gstack).
pub fn load_skills_from_dir(dir: &Path) -> Vec<Skill> {
    if !dir.exists() {
        return Vec::new();
    }

    let mut skills = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                // Direct .md file in the skills directory
                if let Some(skill) = parse_skill(&path) {
                    skills.push(skill);
                }
            } else if path.is_dir() {
                // Sub-directory: look for .md files inside (e.g. cm/SKILL.md)
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.extension().is_some_and(|e| e == "md") {
                            if let Some(skill) = parse_skill(&sub_path) {
                                skills.push(skill);
                            }
                        }
                    }
                }
            }
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Load skills from all standard locations, plus any extra directories.
///
/// Extra directories are scanned after standard locations, so their skills
/// take lower priority (won't override same-name skills from project/global).
pub fn load_all_skills(cwd: &Path, extra_dirs: &[PathBuf]) -> HashMap<String, Skill> {
      let mut map = HashMap::new();
      let home = dirs::home_dir();

      // Project-level skills (highest priority)
      // 1. {cwd}/.aemeath/skills/
      let project_dir = cwd.join(".aemeath").join("skills");
      for skill in load_skills_from_dir(&project_dir) {
          map.insert(skill.name.clone(), skill);
      }

      // Global skills
      if let Some(ref home) = home {
          // 2. ~/.aemeath/skills/
          let aemeath_global = home.join(".aemeath").join("skills");
          for skill in load_skills_from_dir(&aemeath_global) {
              map.entry(skill.name.clone()).or_insert(skill);
          }
          // 3. ~/.agents/skills/
          let agents_global = home.join(".agents").join("skills");
          for skill in load_skills_from_dir(&agents_global) {
              map.entry(skill.name.clone()).or_insert(skill);
          }
      }

      // Extra skill directories from config.json (lowest priority)
      for dir in extra_dirs {
          // Expand `~` to home directory
          let expanded = if dir.starts_with("~") {
              if let Some(ref home) = home {
                  home.join(dir.strip_prefix("~").unwrap_or(dir).strip_prefix("/").unwrap_or(dir))
              } else {
                  dir.clone()
              }
          } else {
              dir.clone()
          };
          for skill in load_skills_from_dir(&expanded) {
              map.entry(skill.name.clone()).or_insert(skill);
          }
      }

      map
  }

/// Load skills and filter based on available tools and other skills.
pub fn load_and_filter_skills(
    cwd: &Path,
    available_tools: &std::collections::HashSet<String>,
    extra_dirs: &[PathBuf],
) -> HashMap<String, Skill> {
    let all_skills = load_all_skills(cwd, extra_dirs);
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
pub fn load_all_skills_cached(cwd: &Path, extra_dirs: &[PathBuf]) -> HashMap<String, Skill> {
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

    let skills = load_all_skills(cwd, extra_dirs);

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
        // dir name == skill name, so no auto-alias needed
        assert!(skill.aliases.is_empty());

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
        // dir name ("my-dir") differs from skill name ("my-skill"), auto-added as alias
        assert_eq!(skill.aliases, vec!["my-dir"]);

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_load_skills_from_subdir() {
        let base = std::env::temp_dir().join("aemeath_test_skill_3");
        let sub = base.join("review");
        std::fs::create_dir_all(&sub).unwrap();

        // Direct .md file
        let direct = base.join("hello.md");
        let mut f = std::fs::File::create(&direct).unwrap();
        write!(f, "---\ndescription: hello skill\n---\nhello").unwrap();

        // Sub-dir .md file
        let sub_file = sub.join("SKILL.md");
        let mut f = std::fs::File::create(&sub_file).unwrap();
        write!(f, "---\ndescription: review skill\n---\nreview").unwrap();

        let skills = load_skills_from_dir(&base);
        assert_eq!(skills.len(), 2, "should load both direct and sub-dir skills");
        assert!(skills.iter().any(|s| s.name == "hello"), "direct skill");
        assert!(skills.iter().any(|s| s.name == "review"), "sub-dir skill");

        std::fs::remove_dir_all(&base).unwrap();
    }
}
