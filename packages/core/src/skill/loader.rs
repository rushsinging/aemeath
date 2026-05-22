use super::{parse_skill, Skill};
use crate::config::paths;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Load all skills from a directory.
///
/// At the top level, scans `.md` files directly and in immediate sub-directories.
/// For sub-directories that contain a `skills/` child, treats them as skill
/// packages and only scans the `skills/` child (supporting
/// `<pkg>/skills/<name>/SKILL.md` convention used by skill packages like superpowers).
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
                let skills_child = path.join("skills");
                if skills_child.is_dir() {
                    // Skill package: scan the `skills/` child and add namespace prefix
                    let pkg_name = path.file_name().map(|n| n.to_string_lossy().to_string());
                    scan_subdir_md(&skills_child, &mut skills, pkg_name.as_deref());
                } else {
                    // Regular skill directory (e.g. cm/SKILL.md)
                    scan_subdir_md(&path, &mut skills, None);
                }
            }
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Scan `.md` files directly in `dir` and in its immediate sub-directories.
/// If `namespace` is provided, skill names are prefixed with `<namespace>:`.
fn scan_subdir_md(dir: &Path, skills: &mut Vec<Skill>, namespace: Option<&str>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                if let Some(mut skill) = parse_skill(&path) {
                    if let Some(ns) = namespace {
                        // Add namespace prefix to skill name and aliases
                        skill.aliases.push(skill.name.clone());
                        skill.name = format!("{ns}:{}", skill.name);
                    }
                    skills.push(skill);
                }
            } else if path.is_dir() {
                // One level deeper: scan .md files inside sub-sub-directories
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.extension().is_some_and(|e| e == "md") {
                            if let Some(mut skill) = parse_skill(&sub_path) {
                                if let Some(ns) = namespace {
                                    skill.aliases.push(skill.name.clone());
                                    skill.name = format!("{ns}:{}", skill.name);
                                }
                                skills.push(skill);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Load skills from all standard locations, plus any extra directories.
///
/// Extra directories are scanned after standard locations, so their skills
/// take lower priority (won't override same-name skills from project/global).
pub fn load_all_skills(cwd: &Path, extra_dirs: &[PathBuf]) -> HashMap<String, Skill> {
    let mut map = HashMap::new();
    let home = dirs::home_dir();

    // Project-level skills (highest priority)
    // 1. {cwd}/.claude/skills/
    let claude_project_dir = paths::project_claude_skills_dir(cwd);
    for skill in load_skills_from_dir(&claude_project_dir) {
        map.insert(skill.name.clone(), skill);
    }

    // 2. {cwd}/.agents/skills/
    let project_dir = paths::project_skills_dir(cwd);
    for skill in load_skills_from_dir(&project_dir) {
        map.entry(skill.name.clone()).or_insert(skill);
    }

    // Global skills
    // 3. ~/.agents/skills/
    let agents_global = paths::global_skills_dir();
    for skill in load_skills_from_dir(&agents_global) {
        map.entry(skill.name.clone()).or_insert(skill);
    }

    // Extra skill directories from aemeath.json (lowest priority)
    for dir in extra_dirs {
        // Expand `~` to home directory
        let expanded = if dir.starts_with("~") {
            if let Some(ref home) = home {
                home.join(
                    dir.strip_prefix("~")
                        .unwrap_or(dir)
                        .strip_prefix("/")
                        .unwrap_or(dir),
                )
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
    let skill_names: std::collections::HashSet<String> = all_skills.keys().cloned().collect();

    all_skills
        .into_iter()
        .filter(|(_, skill)| {
            // Check requires_tools
            if !skill.requires_tools.is_empty()
                && !skill
                    .requires_tools
                    .iter()
                    .all(|t| available_tools.contains(t))
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
#[path = "loader_tests.rs"]
mod tests;
