use crate::guards_engine;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// 规则作用域：`path_prefix` 限定单个相对路径前缀，`path_prefixes` 覆盖
/// 多个并列前缀（把 pattern 完全相同的重复规则合并为一条），
/// `workspace` 覆盖全仓源码。
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    PathPrefix { value: String },
    PathPrefixes { values: Vec<String> },
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
        /// 正则形态的禁模式（切片区间、声明限定标识符等子串无法表达的形态）。
        /// 与子串模式共用 exclusions / allow_marker / 测试区剥离机制。
        #[serde(default)]
        forbidden_regex: Vec<String>,
        #[serde(default)]
        exclusions: Vec<Exclusion>,
        /// 行级豁免标记（如 `allow unsafe_text_op`）：含标记的命中行放行。
        #[serde(default)]
        allow_marker: Option<String>,
        /// 行级豁免标记数组（任一命中即放行，如安全写法函数名/空上限形态）。
        #[serde(default)]
        allow_markers: Vec<String>,
    },
    ConstructionWhitelist {
        symbol: String,
        allowed_paths: Vec<String>,
    },
    /// 计数配比：每文件 denominator 命中行数必须 ≥ numerator 命中行数
    /// （如「外部进程构造数 ≤ session 隔离调用数」）。
    CountRatio {
        numerator_patterns: Vec<String>,
        denominator_patterns: Vec<String>,
        #[serde(default)]
        exclusions: Vec<Exclusion>,
    },
    ForbiddenFileNames {
        forbidden_file_names: Vec<String>,
    },
    /// F-4：workspace 内业务依赖矩阵（crate → 允许依赖的 workspace crate 名）。
    DependencyMatrix {
        business_allow: std::collections::BTreeMap<String, Vec<String>>,
    },
    /// F-14：职责预算（文件行数上限 + 必需拆分文件存在）。
    LineBudget {
        budgets: Vec<LineBudgetEntry>,
        #[serde(default)]
        required_files: Vec<String>,
    },
}

#[derive(Debug, Deserialize, Clone)]
pub struct LineBudgetEntry {
    pub path: String,
    pub max_lines: usize,
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
    /// 豁免条目存量上限（`exclusions` 当前数量不得超过它；只降不升的收缩契约）。
    /// 缺省表示不约束；`guard-registry check` 负责阻断反弹。
    #[serde(default)]
    pub exclusion_baseline: Option<usize>,
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

/// construction_symbols 数据区条目（F-1 样板：跨 BC 构造登记）。
#[derive(Debug, Deserialize, Clone)]
pub struct ConstructionSymbol {
    pub id: String,
    pub symbol: String,
    pub owner_crate: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
}

/// guard 引擎消费的 registry 视图（忽略 entries/budgets 等其他数据区）。
#[derive(Debug, Deserialize)]
pub struct GuardsRegistry {
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub retired_symbols: Vec<RetiredSymbol>,
    #[serde(default)]
    pub construction_symbols: Vec<ConstructionSymbol>,
}

/// 单条违规：规则 id + 相对文件:行 + 修复提示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule_id: String,
    pub location: String,
    pub message: String,
}

pub fn parse_registry(bytes: &[u8]) -> Result<GuardsRegistry> {
    let registry: GuardsRegistry = serde_json::from_slice(bytes)?;
    for rule in &registry.rules {
        let regex_fields: Vec<&[String]> = match &rule.spec {
            RuleSpec::PatternExclusion {
                forbidden_regex, ..
            } => vec![forbidden_regex],
            RuleSpec::CountRatio {
                numerator_patterns,
                denominator_patterns,
                ..
            } => vec![numerator_patterns, denominator_patterns],
            _ => vec![],
        };
        for patterns in regex_fields {
            for pattern in patterns {
                regex::Regex::new(pattern)
                    .with_context(|| format!("规则 {} 的正则非法: {pattern}", rule.id))?;
            }
        }
    }
    Ok(registry)
}

/// 按规则数据对单文件执行断言。scope 不匹配或豁免命中的文件返回空；
/// 生产结构断言（forbidden_segments / facade_whitelist / layer_order /
/// pattern_exclusion）默认跳过测试源（`*_tests.rs` 与 `tests/` 目录）。
pub fn enforce_rule(rule: &Rule, repo_root: &Path, relative_file: &str) -> Result<Vec<Violation>> {
    let context = guards_engine::FileContext::new(repo_root.join(relative_file));
    enforce_rule_with_context(rule, relative_file, &context)
}

/// 共享 [`guards_engine::FileContext`] 版本：一趟文件遍历中多规则复用
/// 同一文件的文本与 use 索引，避免按规则重复读盘 / syn 解析。
pub fn enforce_rule_with_context(
    rule: &Rule,
    relative_file: &str,
    context: &guards_engine::FileContext,
) -> Result<Vec<Violation>> {
    if !scope_matches(&rule.scope, relative_file) {
        return Ok(Vec::new());
    }
    let skips_test_sources = !matches!(
        rule.spec,
        RuleSpec::Layout { .. } | RuleSpec::ForbiddenFileNames { .. }
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
            relative_file,
            context,
            forbidden_segments,
            allow_prefixes,
        ),
        RuleSpec::FacadeWhitelist { allowed_symbols } => {
            enforce_facade_whitelist(rule, relative_file, context, allowed_symbols)
        }
        RuleSpec::LayerOrder { layer_order } => {
            enforce_layer_order(rule, relative_file, context, layer_order)
        }
        RuleSpec::Layout { allowed_entries } => {
            enforce_layout(rule, &rule.scope, relative_file, allowed_entries)
        }
        RuleSpec::PatternExclusion {
            forbidden_patterns,
            forbidden_regex,
            exclusions,
            allow_marker,
            allow_markers,
        } => {
            let mut markers: Vec<&str> = allow_markers.iter().map(String::as_str).collect();
            if let Some(marker) = allow_marker {
                markers.push(marker.as_str());
            }
            enforce_pattern_exclusion(
                rule,
                relative_file,
                context,
                forbidden_patterns,
                forbidden_regex,
                exclusions,
                &markers,
            )
        }
        RuleSpec::ConstructionWhitelist {
            symbol,
            allowed_paths,
        } => enforce_construction_whitelist(rule, relative_file, context, symbol, allowed_paths),
        RuleSpec::CountRatio {
            numerator_patterns,
            denominator_patterns,
            exclusions,
        } => enforce_count_ratio(
            rule,
            relative_file,
            context,
            numerator_patterns,
            denominator_patterns,
            exclusions,
        ),
        RuleSpec::ForbiddenFileNames {
            forbidden_file_names,
        } => enforce_forbidden_file_names(rule, relative_file, forbidden_file_names),
        RuleSpec::DependencyMatrix { .. } => Ok(Vec::new()),
        RuleSpec::LineBudget { .. } => Ok(Vec::new()),
    }
}

fn enforce_forbidden_file_names(
    rule: &Rule,
    relative_file: &str,
    forbidden_file_names: &[String],
) -> Result<Vec<Violation>> {
    let file_name = relative_file.rsplit('/').next().unwrap_or(relative_file);
    if let Some(forbidden) = forbidden_file_names
        .iter()
        .find(|forbidden| *forbidden == file_name)
    {
        return Ok(vec![Violation {
            rule_id: rule.id.clone(),
            location: relative_file.to_owned(),
            message: format!("文件名 `{forbidden}` 被禁止（目录布局约定）"),
        }]);
    }
    Ok(Vec::new())
}

fn scope_matches(scope: &Scope, relative_file: &str) -> bool {
    match scope {
        Scope::PathPrefix { value } => relative_file.starts_with(value.as_str()),
        Scope::PathPrefixes { values } => values
            .iter()
            .any(|value| relative_file.starts_with(value.as_str())),
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
        || file_name.contains("test")
        || relative_file
            .split('/')
            .any(|segment| segment == "tests" || segment.ends_with("_tests"))
}

/// scope 命中的前缀（多前缀取最长命中项，供 layer_order/layout 计算相对层级）；
/// 未命中返回空串。
fn scope_prefix<'a>(scope: &'a Scope, relative_file: &str) -> &'a str {
    match scope {
        Scope::PathPrefix { value } => value.as_str(),
        Scope::PathPrefixes { values } => values
            .iter()
            .filter(|value| relative_file.starts_with(value.as_str()))
            .max_by_key(|value| value.len())
            .map(String::as_str)
            .unwrap_or(""),
        Scope::Workspace => "",
    }
}

fn enforce_forbidden_segments(
    rule: &Rule,
    relative_file: &str,
    context: &guards_engine::FileContext,
    forbidden_segments: &[String],
    allow_prefixes: &[String],
) -> Result<Vec<Violation>> {
    if allow_prefixes
        .iter()
        .any(|prefix| relative_file.starts_with(prefix.as_str()))
    {
        return Ok(Vec::new());
    }
    let Some(index) = context.index() else {
        return Ok(Vec::new());
    };
    let mut violations = Vec::new();
    for use_path in index.production_use_paths() {
        let segments: Vec<&str> = use_path.text.split("::").collect();
        let mut matched: Option<&str> = None;
        for forbidden in forbidden_segments {
            if forbidden.contains("::") {
                // 多段前缀（如 `share::adapter`）：匹配 use 路径的段序列前缀。
                let forbidden_sequence: Vec<&str> = forbidden.split("::").collect();
                if segments.starts_with(&forbidden_sequence[..]) {
                    matched = Some(forbidden);
                    break;
                }
            } else if segments.iter().any(|segment| *segment == forbidden) {
                matched = Some(forbidden);
                break;
            }
        }
        if let Some(forbidden) = matched {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                location: format!("{relative_file}:{}", use_path.line),
                message: format!("use 路径命中禁段 `{forbidden}`：{}", use_path.text),
            });
        }
    }
    Ok(violations)
}

fn enforce_facade_whitelist(
    rule: &Rule,
    relative_file: &str,
    context: &guards_engine::FileContext,
    allowed_symbols: &[String],
) -> Result<Vec<Violation>> {
    if relative_file.rsplit('/').next() != Some("lib.rs") {
        return Ok(Vec::new());
    }
    let Some(index) = context.index() else {
        return Ok(Vec::new());
    };
    let mut violations = Vec::new();
    for reexport in &index.public_reexports {
        if !allowed_symbols.contains(&reexport.symbol) {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                location: format!("{relative_file}:{}", reexport.line),
                message: format!("crate 根导出 `{}` 未在 façade 白名单登记", reexport.symbol),
            });
        }
    }
    Ok(violations)
}

fn enforce_layer_order(
    rule: &Rule,
    relative_file: &str,
    context: &guards_engine::FileContext,
    layer_order: &[String],
) -> Result<Vec<Violation>> {
    let scope_value = scope_prefix(&rule.scope, relative_file);
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
    let Some(index) = context.index() else {
        return Ok(Vec::new());
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
    let scope_value = scope_prefix(scope, relative_file);
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
    relative_file: &str,
    context: &guards_engine::FileContext,
    forbidden_patterns: &[String],
    forbidden_regex: &[String],
    exclusions: &[Exclusion],
    allow_markers: &[&str],
) -> Result<Vec<Violation>> {
    if exclusions
        .iter()
        .any(|exclusion| relative_file.starts_with(exclusion.path.as_str()))
    {
        return Ok(Vec::new());
    }
    let Some(source) = context.text() else {
        return Ok(Vec::new());
    };
    let production = strip_inline_cfg_test_region(source);
    let compiled = compile_regexes(forbidden_regex)?;
    let mut violations = Vec::new();
    for (offset, line) in production.lines().enumerate() {
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        if allow_markers.iter().any(|marker| line.contains(marker)) {
            continue;
        }
        for pattern in forbidden_patterns {
            if line.contains(pattern.as_str()) {
                violations.push(Violation {
                    rule_id: rule.id.clone(),
                    location: format!("{relative_file}:{}", offset + 1),
                    message: format!("命中禁用模式 `{pattern}`"),
                });
            }
        }
        for (pattern, compiled_regex) in forbidden_regex.iter().zip(compiled.iter()) {
            if compiled_regex.is_match(line) {
                violations.push(Violation {
                    rule_id: rule.id.clone(),
                    location: format!("{relative_file}:{}", offset + 1),
                    message: format!("命中禁用正则 `{pattern}`"),
                });
            }
        }
    }
    Ok(violations)
}

/// regex 编译缓存：同一 pattern 全运行期至多编译一次
/// （每条规则 × 每文件调用 enforce，重复编译成本不可忽视）。
fn compile_regexes(patterns: &[String]) -> Result<Vec<std::sync::Arc<regex::Regex>>> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    static CACHE: OnceLock<Mutex<HashMap<String, Arc<regex::Regex>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut compiled = Vec::with_capacity(patterns.len());
    let mut guard = cache.lock().expect("regex cache poisoned");
    for pattern in patterns {
        if !guard.contains_key(pattern) {
            let compiled_regex = regex::Regex::new(pattern)
                .map(Arc::new)
                .with_context(|| format!("非法正则 `{pattern}`"))?;
            guard.insert(pattern.clone(), compiled_regex);
        }
        compiled.push(Arc::clone(&guard[pattern]));
    }
    Ok(compiled)
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
            // 支持 `mod tests {`、`pub(crate) mod tests {`、`pub mod tests {` 等形态。
            let module_decl = trimmed
                .strip_prefix("pub(crate) ")
                .or_else(|| trimmed.strip_prefix("pub "))
                .unwrap_or(trimmed);
            if module_decl.starts_with("mod ") && module_decl.contains('{') {
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
    relative_file: &str,
    context: &guards_engine::FileContext,
    symbol: &str,
    allowed_paths: &[String],
) -> Result<Vec<Violation>> {
    if allowed_paths.iter().any(|allowed| {
        relative_file == allowed.as_str() || relative_file.starts_with(&format!("{allowed}/"))
    }) {
        return Ok(Vec::new());
    }
    let Some(source) = context.text() else {
        return Ok(Vec::new());
    };
    let production = strip_inline_cfg_test_region(source);
    let mut violations = Vec::new();
    for (offset, line) in production.lines().enumerate() {
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

/// 提取行内 `owner::…::wire_xxx(` 形态的限定调用，owner 为链首段
/// （`composition::tools::wire_x` 的 owner 是 composition，非 feature crate 时跳过）。
fn extract_qualified_wire_calls(line: &str) -> Vec<(String, String)> {
    let mut calls = Vec::new();
    let mut search_from = 0;
    while let Some(found) = line[search_from..].find("::wire_") {
        let start = search_from + found;
        // 先回退到 ::wire_ 紧邻的前一段 ident 起点。
        let head = &line[..start];
        let ident_start = head
            .rfind(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
            .map(|boundary| boundary + 1)
            .unwrap_or(0);
        if ident_start >= head.len() || head[ident_start..].contains("::") {
            // 紧邻段不是纯 ident（异常形态），跳过该命中。
            search_from = start + "::wire_".len();
            continue;
        }
        // 回溯整条限定链的起点（连续的 ident:: 序列）。
        let mut chain_begin = ident_start;
        while chain_begin > 0 {
            let previous = line[..chain_begin].trim_end();
            if let Some(segments) = previous.strip_suffix("::") {
                // 前面还有一段 ident::。
                let ident_start = segments
                    .rfind(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                    .map(|boundary| boundary + 1)
                    .unwrap_or(0);
                if ident_start < segments.len()
                    && segments[ident_start..]
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || ch == '_')
                {
                    chain_begin = ident_start;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        let chain = &line[chain_begin..start];
        if let Some(owner_crate) = chain.split("::").next() {
            let symbol_rest = &line[start + 2..];
            let symbol_len = symbol_rest
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .unwrap_or(symbol_rest.len());
            let symbol = &symbol_rest[..symbol_len];
            calls.push((owner_crate.to_owned(), symbol.to_owned()));
        }
        search_from = start + "::wire_".len();
    }
    calls
}

/// 各 feature crate 的 `pub fn wire_*` 公开装配函数定义集（crate → 函数名）。
pub fn collect_wire_definitions(
    repo_root: &std::path::Path,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut definitions = std::collections::BTreeMap::new();
    let features_dir = repo_root.join("agent/features");
    let Ok(entries) = fs::read_dir(&features_dir) else {
        return definitions;
    };
    for entry in entries.flatten() {
        let crate_name = entry.file_name().to_string_lossy().to_string();
        let src_dir = entry.path().join("src");
        let mut wire_set = std::collections::BTreeSet::new();
        let _ = collect_pub_wire_names(&src_dir, &mut wire_set);
        definitions.insert(crate_name, wire_set);
    }
    definitions
}

fn collect_pub_wire_names(
    dir: &std::path::Path,
    wire_set: &mut std::collections::BTreeSet<String>,
) -> std::result::Result<(), std::io::Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_pub_wire_names(&path, wire_set)?;
        } else if name.ends_with(".rs") {
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            for line in source.lines() {
                let trimmed = line.trim_start();
                let Some(rest) = trimmed
                    .strip_prefix("pub fn wire_")
                    .or_else(|| trimmed.strip_prefix("pub async fn wire_"))
                else {
                    continue;
                };
                let symbol_len = rest
                    .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                    .unwrap_or(rest.len());
                if symbol_len > 0 {
                    wire_set.insert(format!("wire_{}", &rest[..symbol_len]));
                }
            }
        }
    }
    Ok(())
}

/// F-1 fail-closed：跨 BC `owner::wire_x` 调用（目标为 owner crate 真实 pub wire
/// 定义且未登记 construction_symbols）违规；同 crate 与非 feature crate 前缀跳过。
pub fn enforce_wire_registration(
    wire_definitions: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    registered: &[(String, String, Vec<String>)],
    relative_file: &str,
    context: &guards_engine::FileContext,
) -> Vec<Violation> {
    if is_test_source(relative_file) {
        return Vec::new();
    }
    let Some(source) = context.text() else {
        return Vec::new();
    };
    let production = strip_inline_cfg_test_region(source);
    // 文件所属 crate：agent/features/<crate>/… → <crate>；composition/share 等 → 目录名。
    let owning_crate = owning_crate_of(relative_file);
    let mut violations = Vec::new();
    for (offset, line) in production.lines().enumerate() {
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        for (owner_crate, symbol) in extract_qualified_wire_calls(line) {
            if owner_crate == owning_crate {
                continue;
            }
            let defined = wire_definitions
                .get(&owner_crate)
                .is_some_and(|symbols| symbols.contains(&symbol));
            if !defined {
                continue;
            }
            // 已登记（owner crate + symbol 匹配且调用文件在允许路径）则放行。
            let registered_hit = registered
                .iter()
                .any(|(entry_owner, entry_symbol, allowed)| {
                    *entry_owner == owner_crate
                        && *entry_symbol == symbol
                        && (allowed.is_empty()
                            || allowed.iter().any(|path| {
                                relative_file == *path
                                    || relative_file.starts_with(&format!("{path}/"))
                            }))
                });
            if registered_hit {
                continue;
            }
            violations.push(Violation {
                rule_id: "construction.cross-bc.fail-closed".to_owned(),
                location: format!("{relative_file}:{}", offset + 1),
                message: format!(
                    "未登记的跨 BC wire 调用 `{owner_crate}::{symbol}`（owner crate: {owner_crate}）；请登记 construction_symbols 或改走注入"
                ),
            });
        }
    }
    violations
}

/// 文件所属 crate 名（agent/features/<crate>/ 与 agent/<crate>/ 两种布局）。
fn owning_crate_of(relative_file: &str) -> String {
    let segments: Vec<&str> = relative_file.split('/').collect();
    if segments.len() >= 3 && segments[0] == "agent" && segments[1] == "features" {
        return segments[2].to_owned();
    }
    if segments.len() >= 2 && segments[0] == "agent" {
        return segments[1].to_owned();
    }
    if segments.len() >= 2 && segments[0] == "packages" {
        return segments[1].to_owned();
    }
    String::new()
}

/// F-4：cargo metadata 的 workspace path 依赖边集合（crate → 依赖 crate 名）。
pub fn workspace_dependency_edges(
    repo_root: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(repo_root)
        .output()
        .with_context(|| "执行 cargo metadata 失败")?;
    if !output.status.success() {
        anyhow::bail!("cargo metadata 退出码非零");
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).with_context(|| "解析 cargo metadata 失败")?;
    let workspace_root = repo_root.to_path_buf();
    let mut edges: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let Some(packages) = metadata["packages"].as_array() else {
        anyhow::bail!("metadata 缺 packages");
    };
    for package in packages {
        let name = package["name"].as_str().unwrap_or_default().to_owned();
        let manifest_path = package["manifest_path"].as_str().unwrap_or_default();
        let Ok(package_root) = std::path::Path::new(manifest_path)
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest 无父目录"))
        else {
            continue;
        };
        let mut dependencies = Vec::new();
        if let Some(deps) = package["dependencies"].as_array() {
            for dependency in deps {
                let Some(dep_path) = dependency["path"].as_str() else {
                    continue;
                };
                let dep_root = std::path::Path::new(dep_path);
                // 仅统计指向 workspace 内的 path 依赖。
                if dep_root.strip_prefix(&workspace_root).is_ok() {
                    if let Some(dep_name) = dependency["name"].as_str() {
                        dependencies.push(dep_name.to_owned());
                    }
                }
            }
        }
        let _ = package_root;
        edges.insert(name, dependencies);
    }
    Ok(edges)
}

/// 校验依赖边集合是否落在业务矩阵内（不在矩阵的 crate 跳过，与原脚本一致）。
pub fn check_dependency_edges(
    business_allow: &std::collections::BTreeMap<String, Vec<String>>,
    edges: &std::collections::BTreeMap<String, Vec<String>>,
) -> Vec<Violation> {
    let workspace_names: std::collections::BTreeSet<&str> =
        edges.keys().map(|name| name.as_str()).collect();
    let mut violations = Vec::new();
    for (name, deps) in edges {
        let Some(allowed) = business_allow.get(name) else {
            continue;
        };
        for dep in deps {
            if workspace_names.contains(dep.as_str()) && !allowed.contains(dep) {
                violations.push(Violation {
                    rule_id: "dependency.workspace-matrix".to_owned(),
                    location: format!("crate:{name}"),
                    message: format!("{name} 不得依赖 {dep}；允许：{allowed:?}"),
                });
            }
        }
    }
    violations
}

/// F-14：行数预算（超限）+ 必需拆分文件存在性。聚合级检查（每规则跑一次，
/// 传入待扫描的源文件清单；行数检查只对 budgets 登记的路径生效）。
pub fn collect_line_budget_violations(
    rule: &Rule,
    repo_root: &std::path::Path,
    _source_files: &[String],
) -> Vec<Violation> {
    let RuleSpec::LineBudget {
        budgets,
        required_files,
    } = &rule.spec
    else {
        return Vec::new();
    };
    let mut violations = Vec::new();
    for budget in budgets {
        let absolute = repo_root.join(&budget.path);
        match fs::read_to_string(&absolute) {
            Ok(source) => {
                let lines = source.lines().count();
                if lines > budget.max_lines {
                    violations.push(Violation {
                        rule_id: rule.id.clone(),
                        location: budget.path.clone(),
                        message: format!("{lines} 行超出职责预算 {}", budget.max_lines),
                    });
                }
            }
            Err(_) => violations.push(Violation {
                rule_id: rule.id.clone(),
                location: budget.path.clone(),
                message: "职责拆分 owner 文件缺失".to_owned(),
            }),
        }
    }
    for required in required_files {
        if !repo_root.join(required).is_file() {
            violations.push(Violation {
                rule_id: rule.id.clone(),
                location: required.clone(),
                message: "必需的职责拆分文件缺失".to_owned(),
            });
        }
    }
    violations
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

/// 计数配比断言：剥离 cfg(test) 后逐文件统计两侧命中行数
/// （一行命中多个 pattern 只计一次），denominator < numerator 即违规。
/// pattern 按 regex 编译（子串字面量即合法 regex），支持 `\b` 词边界精确化。
fn enforce_count_ratio(
    rule: &Rule,
    relative_file: &str,
    context: &guards_engine::FileContext,
    numerator_patterns: &[String],
    denominator_patterns: &[String],
    exclusions: &[Exclusion],
) -> Result<Vec<Violation>> {
    if exclusions
        .iter()
        .any(|exclusion| relative_file.starts_with(exclusion.path.as_str()))
    {
        return Ok(Vec::new());
    }
    let Some(source) = context.text() else {
        return Ok(Vec::new());
    };
    let production = strip_inline_cfg_test_region(source);
    let numerator_regexes = compile_regexes(numerator_patterns)?;
    let denominator_regexes = compile_regexes(denominator_patterns)?;
    let count_hits = |regexes: &[std::sync::Arc<regex::Regex>]| -> usize {
        production
            .lines()
            .filter(|line| {
                let code = line.trim_start();
                !code.starts_with("//") && regexes.iter().any(|regex| regex.is_match(line))
            })
            .count()
    };
    let numerator = count_hits(&numerator_regexes);
    let denominator = count_hits(&denominator_regexes);
    if denominator < numerator {
        return Ok(vec![Violation {
            rule_id: rule.id.clone(),
            location: relative_file.to_owned(),
            message: format!("配比失衡：构造侧 {numerator} 处 > 配对侧 {denominator} 处"),
        }]);
    }
    Ok(Vec::new())
}
