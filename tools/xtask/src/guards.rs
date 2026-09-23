use crate::guards_rules::{self, Rule, Violation};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub use guards_rules::Profile;

/// guard 运行报告：违规统一为 rule_id + 相对文件:行 + 修复提示。
pub struct Report {
    pub violations: Vec<Violation>,
    pub rules_run: usize,
    pub duration: std::time::Duration,
}

impl Report {
    /// 每行格式：`[guard] {rule_id} {location}: {message}`。
    pub fn render(&self) -> String {
        self.violations
            .iter()
            .map(|violation| {
                format!(
                    "[guard] {} {}: {}",
                    violation.rule_id, violation.location, violation.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 读 registry → 枚举 scope 内源码 → 逐规则断言 → 汇总报告。
pub fn run(repo_root: &Path, profile: Profile, rule_filter: Option<&str>) -> Result<Report> {
    let started = Instant::now();
    let registry_path = repo_root.join(".agents/architecture-guard-registry.json");
    let bytes = fs::read(&registry_path)
        .with_context(|| format!("读取 {} 失败", registry_path.display()))?;
    let registry = guards_rules::parse_registry(&bytes)?;

    let selected: Vec<&Rule> = registry
        .rules
        .iter()
        .filter(|rule| match rule_filter {
            Some(rule_id) => rule.id == rule_id,
            None => profile == Profile::Full || rule.profile == guards_rules::Profile::Fast,
        })
        .collect();

    let source_files = collect_source_files(repo_root)?;
    let mut violations = Vec::new();
    for rule in &selected {
        for relative_file in &source_files {
            let file_violations = guards_rules::enforce_rule(rule, repo_root, relative_file)?;
            violations.extend(file_violations);
        }
    }
    violations.sort_by(|left, right| {
        (&left.rule_id, &left.location).cmp(&(&right.rule_id, &right.location))
    });
    violations
        .dedup_by(|left, right| left.rule_id == right.rule_id && left.location == right.location);

    Ok(Report {
        violations,
        rules_run: selected.len(),
        duration: started.elapsed(),
    })
}

/// 按档位筛选规则（fast 档只保留 fast 规则，full 档全跑）。
pub fn profile_rules(rules: &[Rule], profile: Profile) -> Vec<&Rule> {
    rules
        .iter()
        .filter(|rule| profile == Profile::Full || rule.profile == guards_rules::Profile::Fast)
        .collect()
}

/// 枚举仓库内全部 `.rs` 源码相对路径（跳过隐藏目录与 target）。
fn collect_source_files(repo_root: &Path) -> Result<Vec<String>> {
    let mut files = BTreeSet::new();
    walk_source_files(repo_root, repo_root, &mut files)?;
    Ok(files.into_iter().collect())
}

fn walk_source_files(root: &Path, dir: &Path, files: &mut BTreeSet<String>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("读取目录 {} 失败", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path: PathBuf = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            walk_source_files(root, &path, files)?;
        } else if name.ends_with(".rs") {
            if let Ok(relative) = path.strip_prefix(root) {
                files.insert(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}
