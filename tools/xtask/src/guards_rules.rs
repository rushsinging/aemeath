use crate::guards_engine;
use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// 规则作用域：path_prefix 限定相对路径前缀，workspace 覆盖全仓源码。
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    PathPrefix { value: String },
    Workspace,
}

/// fast 档供 Stop hook 快速反馈，full 档供 pre-push 全量门禁。
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Fast,
    Full,
}

/// 断言器数据形态：规则 = 数据行，引擎按断言类型执行。
/// construction_whitelist 承接 check-cross-bc-construction-registry.sh 样板语义。
#[derive(Debug, Deserialize)]
#[serde(tag = "assertion", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleSpec {
    ForbiddenSegments {
        forbidden_segments: Vec<String>,
        #[serde(default)]
        allow_prefixes: Vec<String>,
    },
    FacadeWhitelist {
        allowed_symbols: Vec<String>,
    },
    LayerOrder {
        layer_order: Vec<String>,
    },
    Layout {
        allowed_entries: Vec<String>,
    },
    PatternExclusion {
        forbidden_patterns: Vec<String>,
        #[serde(default)]
        exclusions: Vec<Exclusion>,
    },
    ConstructionWhitelist {
        symbol: String,
        allowed_paths: Vec<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct Exclusion {
    pub path: String,
}

/// registry `rules` 数据区中的一行规则。
#[derive(Debug, Deserialize)]
pub struct Rule {
    pub id: String,
    pub scope: Scope,
    #[serde(flatten)]
    pub spec: RuleSpec,
    #[serde(default)]
    pub reason: String,
    #[serde(default = "default_profile")]
    pub profile: Profile,
}

fn default_profile() -> Profile {
    Profile::Full
}

/// 防复活符号退役名单：纯数据，供 review 对照，不设断言器。
#[derive(Debug, Deserialize)]
pub struct RetiredSymbol {
    pub symbol: String,
    pub retired_by: String,
    pub reason: String,
}

/// guard 引擎消费的 registry 视图（忽略 entries/budgets 等其他数据区）。
#[derive(Debug, Deserialize)]
pub struct GuardsRegistry {
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub retired_symbols: Vec<RetiredSymbol>,
}

/// 单条违规：规则 id + 相对文件:行 + 修复提示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule_id: String,
    pub location: String,
    pub message: String,
}

pub fn parse_registry(bytes: &[u8]) -> Result<GuardsRegistry> {
    Ok(serde_json::from_slice(bytes)?)
}

/// 按规则数据对单文件执行断言。scope 不匹配或豁免命中的文件返回空；
/// 生产结构断言（forbidden_segments / facade_whitelist / layer_order /
/// pattern_exclusion）默认跳过测试源（`*_tests.rs` 与 `tests/` 目录）。
pub fn enforce_rule(rule: &Rule, repo_root: &Path, relative_file: &str) -> Result<Vec<Violation>> {
    if !scope_matches(&rule.scope, relative_file) {
        return Ok(Vec::new());
    }
    let absolute = repo_root.join(relative_file);
    if !absolute.is_file() {
        return Ok(Vec::new());
    }
    let skips_test_sources = !matches!(
        rule.spec,
        RuleSpec::Layout { .. } | RuleSpec::ConstructionWhitelist { .. }
    );
    if skips_test_sources && is_test_source(relative_file) {
        return Ok(Vec::new());
    }
    match &rule.spec {
        RuleSpec::ForbiddenSegments {
            forbidden_segments,
            allow_prefixes,
        } => enforce_forbidden_segments(
            rule,
            repo_root,
            relative_file,
            forbidden_segments,
            allow_prefixes,
        ),
        RuleSpec::FacadeWhitelist { allowed_symbols } => {
            enforce_facade_whitelist(rule, &absolute, relative_file, allowed_symbols)
        }
        RuleSpec::LayerOrder { layer_order } => {
            enforce_layer_order(rule, &absolute, relative_file, layer_order)
        }
        RuleSpec::Layout { allowed_entries } => {
            enforce_layout(rule, &rule.scope, relative_file, allowed_entries)
        }
        RuleSpec::PatternExclusion {
            forbidden_patterns,
            exclusions,
        } => enforce_pattern_exclusion(
            rule,
            &absolute,
            relative_file,
            forbidden_patterns,
            exclusions,
        ),
        RuleSpec::ConstructionWhitelist {
            symbol,
            allowed_paths,
        } => enforce_construction_whitelist(rule, &absolute, relative_file, symbol, allowed_paths),
    }
}

fn scope_matches(scope: &Scope, relative_file: &str) -> bool {
    match scope {
        Scope::PathPrefix { value } => relative_file.starts_with(value.as_str()),
        Scope::Workspace => true,
    }
}

/// 测试源判定：分离测试文件（`*_tests.rs`、纯 `tests.rs` 模块文件）与
/// 测试目录（`tests/` 或任意 `*_tests/` 命名目录，如 `scenario_tests/`）。
fn is_test_source(relative_file: &str) -> bool {
    let file_name = relative_file.rsplit('/').next().unwrap_or(relative_file);
    file_name.ends_with("_tests.rs")
        || file_name.ends_with("_test.rs")
        || file_name == "tests.rs"
        || relative_file
            .split('/')
            .any(|segment| segment == "tests" || segment.ends_with("_tests"))
}

fn scope_prefix(scope: &Scope) -> &str {
    match scope {
        Scope::PathPrefix { value } => value.as_str(),
        Scope::Workspace => "",
    }
}

fn enforce_forbidden_segments(
    rule: &Rule,
    repo_root: &Path,
    relative_file: &str,
    forbidden_segments: &[String],
    allow_prefixes: &[String],
) -> Result<Vec<Violation>> {
    if allow_prefixes
        .iter()
        .any(|prefix| relative_file.starts_with(prefix.as_str()))
    {
        return Ok(Vec::new());
    }
    let index = match guards_engine::index_file(&repo_root.join(relative_file)) {
        Ok(index) => index,
        Err(_) => return Ok(Vec::new()),
    };
    let mut violations = Vec::new();
    for use_path in index.production_use_paths() {
        let segments: Vec<&str> = use_path.text.split("::").collect();
        if let Some(segment) = segments.iter().find(|segment| {
            forbidden_segments
                .iter()
                .any(|forbidden| forbidden == *segment)
        }) {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                location: format!("{relative_file}:{}", use_path.line),
                message: format!("use 路径含禁段 `{segment}`：{}", use_path.text),
            });
        }
    }
    Ok(violations)
}

fn enforce_facade_whitelist(
    rule: &Rule,
    absolute: &Path,
    relative_file: &str,
    allowed_symbols: &[String],
) -> Result<Vec<Violation>> {
    if absolute
        .file_name()
        .map(|name| name != "lib.rs")
        .unwrap_or(true)
    {
        return Ok(Vec::new());
    }
    let index = match guards_engine::index_file(absolute) {
        Ok(index) => index,
        Err(_) => return Ok(Vec::new()),
    };
    let mut violations = Vec::new();
    for symbol in &index.public_reexports {
        if !allowed_symbols.contains(symbol) {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                location: relative_file.to_owned(),
                message: format!("crate 根导出 `{symbol}` 未在 façade 白名单登记"),
            });
        }
    }
    Ok(violations)
}

fn enforce_layer_order(
    rule: &Rule,
    absolute: &Path,
    relative_file: &str,
    layer_order: &[String],
) -> Result<Vec<Violation>> {
    let scope_value = scope_prefix(&rule.scope);
    let remainder = relative_file
        .strip_prefix(scope_value)
        .unwrap_or(relative_file)
        .trim_start_matches('/');
    let components: Vec<&str> = remainder
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let Some(first) = components.first() else {
        return Ok(Vec::new());
    };
    let Some(file_layer) = layer_order.iter().position(|layer| layer == first) else {
        return Ok(Vec::new());
    };
    let index = match guards_engine::index_file(absolute) {
        Ok(index) => index,
        Err(_) => return Ok(Vec::new()),
    };
    let mut violations = Vec::new();
    for use_path in index.production_use_paths() {
        let segments: Vec<&str> = use_path.text.split("::").collect();
        if segments.first().copied() != Some("crate") {
            continue;
        }
        let Some(dependency) = segments.get(1) else {
            continue;
        };
        if let Some(dependency_layer) = layer_order.iter().position(|layer| layer == dependency) {
            if dependency_layer > file_layer {
                violations.push(Violation {
                    rule_id: rule.id.clone(),
                    location: format!("{relative_file}:{}", use_path.line),
                    message: format!("内层 `{first}` 依赖外层 `{dependency}`：{}", use_path.text),
                });
            }
        }
    }
    Ok(violations)
}

fn enforce_layout(
    rule: &Rule,
    scope: &Scope,
    relative_file: &str,
    allowed_entries: &[String],
) -> Result<Vec<Violation>> {
    let scope_value = scope_prefix(scope);
    let remainder = relative_file
        .strip_prefix(scope_value)
        .unwrap_or(relative_file)
        .trim_start_matches('/');
    let mut components = remainder.split('/');
    let Some(entry) = components.next() else {
        return Ok(Vec::new());
    };
    if allowed_entries.iter().any(|allowed| allowed == entry) {
        return Ok(Vec::new());
    }
    Ok(vec![Violation {
        rule_id: rule.id.clone(),
        location: relative_file.to_owned(),
        message: format!("scope 下条目 `{entry}` 未在布局白名单登记"),
    }])
}

fn enforce_pattern_exclusion(
    rule: &Rule,
    absolute: &Path,
    relative_file: &str,
    forbidden_patterns: &[String],
    exclusions: &[Exclusion],
) -> Result<Vec<Violation>> {
    if exclusions
        .iter()
        .any(|exclusion| relative_file.starts_with(exclusion.path.as_str()))
    {
        return Ok(Vec::new());
    }
    let source = match fs::read_to_string(absolute) {
        Ok(source) => source,
        Err(_) => return Ok(Vec::new()),
    };
    let production = strip_inline_cfg_test_region(&source);
    let mut violations = Vec::new();
    for (offset, line) in production.lines().enumerate() {
        for pattern in forbidden_patterns {
            if line.contains(pattern.as_str()) {
                violations.push(Violation {
                    rule_id: rule.id.clone(),
                    location: format!("{relative_file}:{}", offset + 1),
                    message: format!("命中禁用模式 `{pattern}`"),
                });
            }
        }
    }
    Ok(violations)
}

/// 剥离内联 `#[cfg(test)] mod name { ... }` 区块（保留区块前的生产行号语义：
/// 以占位空行维持总行数，保证违规定位行号与原文件一致）。
fn strip_inline_cfg_test_region(source: &str) -> String {
    let mut output_lines: Vec<String> = Vec::new();
    let mut pending_test_attr = false;
    let mut test_block_depth: Option<i32> = None;
    let mut depth: i32 = 0;
    for line in source.lines() {
        let trimmed = line.trim();
        let mut blanked = false;
        if trimmed == "#[cfg(test)]" {
            pending_test_attr = true;
            blanked = true;
        } else if test_block_depth.is_none() && pending_test_attr {
            if trimmed.starts_with("mod ") && trimmed.contains('{') {
                test_block_depth = Some(depth);
            } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                pending_test_attr = false;
            }
            if test_block_depth.is_some() {
                blanked = true;
            }
        } else if let Some(start_depth) = test_block_depth {
            if depth <= start_depth {
                test_block_depth = None;
            } else {
                blanked = true;
            }
        }
        output_lines.push(if blanked {
            String::new()
        } else {
            line.to_owned()
        });
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
    }
    output_lines.join("\n")
}

fn enforce_construction_whitelist(
    rule: &Rule,
    absolute: &Path,
    relative_file: &str,
    symbol: &str,
    allowed_paths: &[String],
) -> Result<Vec<Violation>> {
    if allowed_paths
        .iter()
        .any(|allowed| relative_file == allowed.as_str())
    {
        return Ok(Vec::new());
    }
    let source = match fs::read_to_string(absolute) {
        Ok(source) => source,
        Err(_) => return Ok(Vec::new()),
    };
    let mut violations = Vec::new();
    for (offset, line) in source.lines().enumerate() {
        if contains_symbol(line, symbol) {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                location: format!("{relative_file}:{}", offset + 1),
                message: format!("符号 `{symbol}` 只准出现在登记的构造点"),
            });
        }
    }
    Ok(violations)
}

/// 词边界匹配：避免 `AdapterX` 误命中 `Adapter`。
fn contains_symbol(line: &str, symbol: &str) -> bool {
    let mut search_from = 0;
    while let Some(found) = line[search_from..].find(symbol) {
        let start = search_from + found;
        let end = start + symbol.len();
        let before_is_boundary = line[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_alphanumeric() && ch != '_');
        let after_is_boundary = line[end..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_alphanumeric() && ch != '_');
        if before_is_boundary && after_is_boundary {
            return true;
        }
        search_from = end;
    }
    false
}
