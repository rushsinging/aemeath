use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CLASSIFICATIONS: [&str; 5] = [
    "target_capability_policy",
    "target_hexagonal_policy",
    "scope_exclusion",
    "false_positive_suppression",
    "migration_exception",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    version: u32,
    budgets: Budgets,
    entries: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Budgets {
    repository_migration_debt: usize,
    #[serde(default)]
    modules: BTreeMap<String, usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    guard: String,
    module: String,
    scope: Scope,
    classification: String,
    owner: String,
    reason: String,
    tracking_issue: Option<u64>,
    introduced_baseline: String,
    exit_condition: String,
    status: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    kind: String,
    value: String,
}

#[derive(Debug)]
pub struct RegistryReport {
    pub migration_debt: usize,
    pub by_classification: BTreeMap<String, usize>,
    by_module: BTreeMap<String, usize>,
    by_guard: BTreeMap<String, usize>,
    by_kind: BTreeMap<String, usize>,
    by_status: BTreeMap<String, usize>,
    entries: Vec<ReportEntry>,
}

#[derive(Debug)]
struct ReportEntry {
    id: String,
    classification: String,
    module: String,
}

impl RegistryReport {
    pub fn render(&self) -> String {
        let mut output = format!("migration_debt: {}\n", self.migration_debt);
        for (classification, count) in &self.by_classification {
            output.push_str(&format!("classification.{classification}: {count}\n"));
        }
        for (module, count) in &self.by_module {
            output.push_str(&format!("module.{module}: {count}\n"));
        }
        for (guard, count) in &self.by_guard {
            output.push_str(&format!("guard.{guard}: {count}\n"));
        }
        for (kind, count) in &self.by_kind {
            output.push_str(&format!("kind.{kind}: {count}\n"));
        }
        for (status, count) in &self.by_status {
            output.push_str(&format!("lifecycle.{status}: {count}\n"));
        }
        for entry in &self.entries {
            output.push_str(&format!(
                "{}\t{}\t{}\n",
                entry.id, entry.classification, entry.module
            ));
        }
        output
    }
}

pub fn validate_str(input: &str) -> Result<RegistryReport> {
    let registry: Registry = serde_json::from_str(input).context("解析架构 Guard 注册表失败")?;
    validate_registry(&registry)
}

pub fn check_workspace(root: &Path, report_output: Option<&Path>) -> Result<RegistryReport> {
    let registry_path = root.join(".agents/architecture-guard-registry.json");
    let input = fs::read_to_string(&registry_path)
        .with_context(|| format!("读取 {} 失败", registry_path.display()))?;
    let registry: Registry = serde_json::from_str(&input).context("解析架构 Guard 注册表失败")?;
    let report = validate_registry(&registry)?;
    validate_scopes(root, &registry)?;
    validate_tracking_issues(root, &registry)?;
    validate_registry_references(root, &registry)?;
    validate_script_exclusions(root, &registry)?;
    if let Some(path) = report_output {
        fs::write(path, report.render())
            .with_context(|| format!("写入 {} 失败", path.display()))?;
    }
    Ok(report)
}

fn validate_registry(registry: &Registry) -> Result<RegistryReport> {
    let mut violations = Vec::new();
    if registry.version != 1 {
        violations.push(format!("不支持的注册表版本 {}", registry.version));
    }
    let mut ids = BTreeSet::new();
    let mut by_classification = BTreeMap::new();
    let mut by_module = BTreeMap::new();
    let mut by_guard = BTreeMap::new();
    let mut by_kind = BTreeMap::new();
    let mut by_status = BTreeMap::new();
    let mut module_debt: BTreeMap<&str, usize> = BTreeMap::new();
    let mut report_entries = Vec::new();

    for entry in &registry.entries {
        if entry.id.trim().is_empty() {
            violations.push("stable id 不能为空".to_owned());
        } else if !ids.insert(entry.id.as_str()) {
            violations.push(format!("stable id 重复: {}", entry.id));
        }
        if !entry.id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '-')
        }) {
            violations.push(format!("{}: stable id 格式非法", entry.id));
        }
        if !CLASSIFICATIONS.contains(&entry.classification.as_str()) {
            violations.push(format!(
                "{}: classification 非法: {}",
                entry.id, entry.classification
            ));
        }
        if !matches!(
            entry.scope.kind.as_str(),
            "path" | "path_prefix" | "symbol" | "pattern"
        ) {
            violations.push(format!(
                "{}: scope.kind 非法: {}",
                entry.id, entry.scope.kind
            ));
        }
        if entry.status != "active" {
            violations.push(format!(
                "{}: status 必须为 active，发现 {}",
                entry.id, entry.status
            ));
        }
        if entry.tracking_issue == Some(0) {
            violations.push(format!("{}: tracking_issue 必须为正整数", entry.id));
        }
        for (field, value) in [
            ("guard", entry.guard.as_str()),
            ("module", entry.module.as_str()),
            ("owner", entry.owner.as_str()),
            ("reason", entry.reason.as_str()),
            ("introduced_baseline", entry.introduced_baseline.as_str()),
            ("exit_condition", entry.exit_condition.as_str()),
            ("status", entry.status.as_str()),
            ("scope.kind", entry.scope.kind.as_str()),
            ("scope.value", entry.scope.value.as_str()),
        ] {
            if value.trim().is_empty() {
                violations.push(format!("{}: {field} 不能为空", entry.id));
            }
        }
        if entry.classification == "migration_exception" {
            if entry.tracking_issue.is_none() {
                violations.push(format!("{}: tracking_issue 不能为空", entry.id));
            }
            *module_debt.entry(entry.module.as_str()).or_default() += 1;
        }
        *by_classification
            .entry(entry.classification.clone())
            .or_insert(0) += 1;
        *by_module.entry(entry.module.clone()).or_insert(0) += 1;
        *by_guard.entry(entry.guard.clone()).or_insert(0) += 1;
        *by_kind.entry(entry.scope.kind.clone()).or_insert(0) += 1;
        *by_status.entry(entry.status.clone()).or_insert(0) += 1;
        report_entries.push(ReportEntry {
            id: entry.id.clone(),
            classification: entry.classification.clone(),
            module: entry.module.clone(),
        });
    }

    let migration_debt = module_debt.values().sum();
    if migration_debt > registry.budgets.repository_migration_debt {
        violations.push(format!(
            "仓库 migration debt {} 超过预算 {}",
            migration_debt, registry.budgets.repository_migration_debt
        ));
    }
    for (module, count) in module_debt {
        let budget = registry.budgets.modules.get(module).copied().unwrap_or(0);
        if count > budget {
            violations.push(format!(
                "模块 {module} migration debt {count} 超过预算 {budget}"
            ));
        }
    }
    if !violations.is_empty() {
        anyhow::bail!(violations.join("\n"));
    }
    report_entries.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(RegistryReport {
        migration_debt,
        by_classification,
        by_module,
        by_guard,
        by_kind,
        by_status,
        entries: report_entries,
    })
}

fn validate_scopes(root: &Path, registry: &Registry) -> Result<()> {
    let mut violations = Vec::new();
    for entry in &registry.entries {
        if !matches!(entry.scope.kind.as_str(), "path" | "path_prefix") {
            continue;
        }
        let relative = Path::new(&entry.scope.value);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            violations.push(format!("{}: scope 必须位于仓库内", entry.id));
            continue;
        }
        let path = root.join(relative);
        let exists = if entry.scope.kind == "path_prefix" {
            path.is_dir()
        } else {
            path.exists()
        };
        if !exists {
            violations.push(format!("{}: stale scope {}", entry.id, entry.scope.value));
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(violations.join("\n"))
    }
}

fn validate_tracking_issues(root: &Path, registry: &Registry) -> Result<()> {
    if std::env::var_os("AEMEATH_GUARD_REGISTRY_SKIP_ISSUE_CHECK").is_some() {
        return Ok(());
    }
    let mut issues = BTreeSet::new();
    for entry in &registry.entries {
        if entry.classification == "migration_exception" {
            if let Some(issue) = entry.tracking_issue {
                issues.insert(issue);
            }
        }
    }
    let mut violations = Vec::new();
    for issue in issues {
        let output = Command::new("gh")
            .args([
                "issue",
                "view",
                &issue.to_string(),
                "--repo",
                "rushsinging/aemeath",
                "--json",
                "state",
                "--jq",
                ".state",
            ])
            .current_dir(root)
            .output()
            .with_context(|| format!("查询 tracking issue #{issue} 失败"))?;
        if !output.status.success() {
            anyhow::bail!(
                "查询 tracking issue #{issue} 失败: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let state = String::from_utf8_lossy(&output.stdout);
        if state.trim() != "OPEN" {
            violations.push(format!(
                "tracking issue #{issue} 已关闭但 migration exception 仍存在"
            ));
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(violations.join("\n"))
    }
}

fn validate_registry_references(root: &Path, registry: &Registry) -> Result<()> {
    let hooks = root.join(".agents/hooks");
    if !hooks.exists() {
        return Ok(());
    }
    let mut violations = Vec::new();
    for entry in &registry.entries {
        let guard_path = hooks.join(&entry.guard);
        if !guard_path.is_file() {
            violations.push(format!("{}: guard 不存在: {}", entry.id, entry.guard));
            continue;
        }
        let source = fs::read_to_string(&guard_path)?;
        let exact_marker = format!("guard-registry:{}", entry.id);
        let references = source
            .lines()
            .filter(|line| registry_reference(line) == Some(entry.id.as_str()))
            .count();
        if references == 0 || !source.contains(&exact_marker) {
            violations.push(format!(
                "{}: 注册项未被指定 Guard {} 引用",
                entry.id, entry.guard
            ));
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(violations.join("\n"))
    }
}

fn validate_script_exclusions(root: &Path, registry: &Registry) -> Result<()> {
    let hooks = root.join(".agents/hooks");
    if !hooks.exists() {
        return Ok(());
    }
    let registered: BTreeMap<&str, &Entry> = registry
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    let mut scripts = collect_scripts(&hooks)?;
    scripts.sort();
    let mut violations = Vec::new();
    for script in scripts {
        let source = fs::read_to_string(&script)?;
        for (index, line) in source.lines().enumerate() {
            if !is_exclusion_line(line) {
                continue;
            }
            let Some(id) =
                registry_reference(line).or_else(|| previous_registry_reference(&source, index))
            else {
                violations.push(format!(
                    "{}:{}: 未登记隐式排除: {}",
                    script.strip_prefix(root).unwrap_or(&script).display(),
                    index + 1,
                    line.trim()
                ));
                continue;
            };
            let Some(entry) = registered.get(id) else {
                violations.push(format!(
                    "{}:{}: 未知 guard registry id {id}",
                    script.strip_prefix(root).unwrap_or(&script).display(),
                    index + 1
                ));
                continue;
            };
            let guard = script
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if entry.guard != guard {
                violations.push(format!(
                    "{}:{}: registry id {id} 属于 {} 而非 {guard}",
                    script.strip_prefix(root).unwrap_or(&script).display(),
                    index + 1,
                    entry.guard
                ));
            }
            if !matches!(
                entry.classification.as_str(),
                "scope_exclusion" | "false_positive_suppression" | "migration_exception"
            ) {
                violations.push(format!("{id}: Target policy 不得批准文本排除"));
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(violations.join("\n"))
    }
}

fn previous_registry_reference(source: &str, index: usize) -> Option<&str> {
    let previous = source.lines().nth(index.checked_sub(1)?)?;
    registry_reference(previous)
}

fn registry_reference(line: &str) -> Option<&str> {
    let marker = "guard-registry:";
    let tail = line.split_once(marker)?.1.trim_start();
    let id = tail
        .split(|character: char| character.is_whitespace() || matches!(character, '"' | '\''))
        .next()?;
    (!id.is_empty()).then_some(id)
}

fn is_exclusion_line(line: &str) -> bool {
    let line = line.trim();
    !line.starts_with('#')
        && (line.contains("grep -v")
            || line.contains("grep --invert-match")
            || line.contains("rg -v")
            || line.contains("rg --invert-match")
            || line.contains("--glob '!")
            || line.contains("--glob=!")
            || line.contains("--exclude=")
            || line.contains("--exclude-dir=")
            || line.trim_start().starts_with("EXEMPT_FILES=")
            || line.trim_start().ends_with("MIGRATION_EXCEPTIONS = {")
            || line.contains("if [[ \"$line\" == *\"allow tea_side_effect\"*")
            || line.contains("if [[ \"$line\" == *\"allow unsafe_text_op\"*"))
}

fn collect_scripts(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut scripts = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "sh") {
            scripts.push(path);
        }
    }
    Ok(scripts)
}
