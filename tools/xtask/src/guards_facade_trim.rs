//! #1696 façade 下架工具：AST 级死导出计算与原子应用。
//!
//! 三类消费面统一计算：跨 crate（含集成契约测试）消费、crate 内经根折返
//! （需改写为真实模块路径后即可下架）、crate 内真实路径消费（不影响 façade）。

use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// 一个 crate 的 façade 分析结果。
pub struct FacadeReport {
    pub crate_name: String,
    /// lib.rs 导出叶（含来源模块路径，供下架/折返改写）。
    pub exports: BTreeMap<String, String>,
    /// 跨 crate + owner 集成测试消费的符号（保留）。
    pub cross_consumed: BTreeSet<String>,
    /// crate 内经根折返消费的符号（改写后可下架）。
    pub internal_root_consumed: BTreeSet<String>,
}

/// lib.rs 导出叶 → 来源模块（如 `wire_x` → `composition`）。
fn export_leaves_with_module(lib_source: &str) -> BTreeMap<String, String> {
    let mut exports = BTreeMap::new();
    let Ok(syntax) = syn::parse_file(lib_source) else {
        return exports;
    };
    for item in syntax.items {
        let syn::Item::Use(use_item) = item else {
            continue;
        };
        if !matches!(use_item.vis, syn::Visibility::Public(_)) {
            continue;
        }
        let module = module_prefix_of(&use_item.tree);
        collect_use_leaves(&use_item.tree, "", &module, &mut exports);
    }
    exports
}

/// `pub use domain::{A, B}` 的模块前缀（多段取首段之后的完整路径）。
fn module_prefix_of(tree: &syn::UseTree) -> String {
    match tree {
        syn::UseTree::Path(segment) => {
            let nested = module_prefix_of(&segment.tree);
            if nested.is_empty() {
                segment.ident.to_string()
            } else {
                format!("{}::{}", segment.ident, nested)
            }
        }
        syn::UseTree::Group(_) => String::new(),
        _ => String::new(),
    }
}

/// 收集 use 树叶子。`module` 为当前已确定前缀；Path 段在其上追加
/// （顶层前缀由 module_prefix_of 消化，避免双重拼接）。
fn collect_use_leaves(
    tree: &syn::UseTree,
    prefix: &str,
    module: &str,
    exports: &mut BTreeMap<String, String>,
) {
    match tree {
        syn::UseTree::Path(segment) => {
            let path = if prefix.is_empty() {
                segment.ident.to_string()
            } else {
                format!("{prefix}::{}", segment.ident)
            };
            collect_use_leaves(&segment.tree, &path, module, exports);
        }
        syn::UseTree::Name(name) => {
            exports.insert(name.ident.to_string(), module.to_owned());
        }
        syn::UseTree::Rename(rename) => {
            exports.insert(rename.rename.to_string(), module.to_owned());
        }
        syn::UseTree::Glob(_) => {}
        syn::UseTree::Group(group) => {
            for nested in &group.items {
                collect_use_leaves(nested, module, module, exports);
            }
        }
    }
}

/// 词边界符号扫描（消费面判定；Rust 级实现避免 shell 差异）。
fn file_mentions_symbol(source: &str, symbol: &str) -> bool {
    let bytes = source.as_bytes();
    let mut search_from = 0;
    while let Some(found) = source[search_from..].find(symbol) {
        let start = search_from + found;
        let end = start + symbol.len();
        let before_ok =
            start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        let after_ok =
            end >= bytes.len() || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
        if before_ok && after_ok {
            return true;
        }
        search_from = end;
    }
    false
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if name.ends_with(".rs") {
            files.push(path);
        }
    }
}

/// 分析单 crate：三类消费面。
pub fn analyze_crate(repo_root: &Path, crate_name: &str) -> Result<FacadeReport> {
    let lib_path = repo_root.join(format!("agent/features/{crate_name}/src/lib.rs"));
    let lib_source = fs::read_to_string(&lib_path)?;
    let exports = export_leaves_with_module(&lib_source);
    let symbols: Vec<&String> = exports.keys().collect();

    // 面 1：跨 crate + 外部工程消费（含 owner tests/ 集成契约）。
    let mut consumer_dirs: Vec<PathBuf> = vec![
        repo_root.join("agent/composition/src"),
        repo_root.join("apps/cli/src"),
        repo_root.join("packages/sdk/src"),
        repo_root.join("agent/shared/src"),
        repo_root.join(format!("agent/features/{crate_name}/tests")),
    ];
    for other in CRATES {
        if other != crate_name {
            // 其他 crate 的 src 与 tests/（集成契约测试跨 crate 消费 façade）。
            consumer_dirs.push(repo_root.join(format!("agent/features/{other}/src")));
            consumer_dirs.push(repo_root.join(format!("agent/features/{other}/tests")));
        }
    }
    let mut cross_consumed = BTreeSet::new();
    for symbol in &symbols {
        let mut hit = false;
        for dir in &consumer_dirs {
            let mut files = Vec::new();
            collect_rust_files(dir, &mut files);
            for file in &files {
                if let Ok(source) = fs::read_to_string(file) {
                    if file_mentions_symbol(&source, symbol) {
                        hit = true;
                        break;
                    }
                }
            }
            if hit {
                break;
            }
        }
        if hit {
            cross_consumed.insert((*symbol).clone());
        }
    }

    // 面 2：crate 内经根折返（crate::Sym 限定或 use crate::{Sym} 组内裸名）。
    let mut internal_root_consumed = BTreeSet::new();
    let mut internal_files = Vec::new();
    collect_rust_files(
        &repo_root.join(format!("agent/features/{crate_name}/src")),
        &mut internal_files,
    );
    for symbol in &symbols {
        let mut hit = false;
        for file in &internal_files {
            if file.ends_with("lib.rs") {
                continue;
            }
            let Ok(source) = fs::read_to_string(file) else {
                continue;
            };
            if internal_root_consumes(&source, symbol) {
                hit = true;
                break;
            }
        }
        if hit {
            internal_root_consumed.insert((*symbol).clone());
        }
    }

    Ok(FacadeReport {
        crate_name: crate_name.to_owned(),
        exports,
        cross_consumed,
        internal_root_consumed,
    })
}

/// 内部文件是否经 crate 根折返消费该符号。
fn internal_root_consumes(source: &str, symbol: &str) -> bool {
    // 限定形态：crate::Symbol（后不跟 ::，排除 crate::domain::… 中的模块段）。
    let qualified = format!("crate::{symbol}");
    let mut search_from = 0;
    while let Some(found) = source[search_from..].find(&qualified) {
        let end = search_from + found + qualified.len();
        let bytes = source.as_bytes();
        let before_ok = search_from + found == 0
            || !(bytes[search_from + found - 1].is_ascii_alphanumeric()
                || bytes[search_from + found - 1] == b'_');
        // 符号本身是导出符号：crate::X:: 无论后随模块段还是枚举变体都算经根折返。
        if before_ok {
            return true;
        }
        search_from = end;
    }
    // 组形态：use crate::{ … Symbol … };（组内裸名或 Self 路径首段）。
    if let Some(use_start) = source.find("use crate::{") {
        if let Some(semi) = source[use_start..].find(';') {
            let group = &source[use_start..use_start + semi];
            let mut token = String::new();
            for (offset, ch) in group.char_indices() {
                if ch.is_alphanumeric() || ch == '_' {
                    token.push(ch);
                } else {
                    if token == symbol && !path_context(group, offset.saturating_sub(token.len())) {
                        return true;
                    }
                    token.clear();
                }
            }
        }
    }
    false
}

/// 组内 token 前是否紧邻 `::`（是则属于路径段而非根裸名）。
fn path_context(group: &str, token_start: usize) -> bool {
    group[..token_start].ends_with("::")
}

pub const CRATES: [&str; 12] = [
    "audit", "config", "context", "hook", "memory", "policy", "project", "provider", "runtime",
    "storage", "task", "tools",
];

/// 死集 = 导出 − 跨面消费 − 内部折返（折返先改写后也可下架，
/// 死集计算保守地只含两类面皆无的符号；折返集单列由调用方决定改写顺序）。
pub fn dead_exports(report: &FacadeReport) -> BTreeSet<String> {
    report
        .exports
        .keys()
        .filter(|symbol| {
            !report.cross_consumed.contains(*symbol)
                && !report.internal_root_consumed.contains(*symbol)
        })
        .cloned()
        .collect()
}

/// 下架：从 lib.rs 的 pub use 树中剔除死符号（语句级重建，含空组清理）。
pub fn apply_trim(repo_root: &Path, crate_name: &str, dead: &BTreeSet<String>) -> Result<usize> {
    let lib_path = repo_root.join(format!("agent/features/{crate_name}/src/lib.rs"));
    let source = fs::read_to_string(&lib_path)?;
    let mut removed = 0;
    let mut output_lines: Vec<String> = Vec::new();
    let mut current_use: Option<Vec<String>> = None;
    for line in source.lines() {
        let trimmed = line.trim_start();
        let is_use = trimmed.starts_with("pub use ") || trimmed.starts_with("pub use{");
        if current_use.is_some() || is_use {
            current_use
                .get_or_insert_with(Vec::new)
                .push(line.to_owned());
            let joined: String = current_use.as_ref().expect("use buffer").join("\n");
            if joined.trim_end().ends_with(';') {
                let statement = joined;
                if statement.trim_start().starts_with("pub use") {
                    let trimmed_statement = trim_use_statement(&statement, dead, &mut removed);
                    for out_line in trimmed_statement.lines() {
                        output_lines.push(out_line.to_owned());
                    }
                } else {
                    output_lines.push(statement);
                }
                current_use = None;
            }
        } else {
            output_lines.push(line.to_owned());
        }
    }
    let mut new_source = output_lines.join("\n");
    if source.ends_with('\n') && !new_source.ends_with('\n') {
        new_source.push('\n');
    }
    fs::write(&lib_path, new_source)?;
    Ok(removed)
}

/// 从单条 pub use 语句剔除死符号（词边界；仅语句内操作）。
fn trim_use_statement(statement: &str, dead: &BTreeSet<String>, removed: &mut usize) -> String {
    let mut result = statement.to_owned();
    for symbol in dead {
        let pattern = format!("{symbol},");
        let mut replaced = String::new();
        let mut rest = result.as_str();
        while let Some(found) = rest.find(&pattern) {
            let before = &rest[..found];
            let bytes = before.as_bytes();
            let boundary_ok = before.is_empty()
                || !(bytes[bytes.len() - 1].is_ascii_alphanumeric()
                    || bytes[bytes.len() - 1] == b'_'
                    || bytes[bytes.len() - 1] == b':');
            if boundary_ok {
                replaced.push_str(before);
                rest = &rest[found + pattern.len()..];
                *removed += 1;
            } else {
                replaced.push_str(&rest[..found + pattern.len()]);
                rest = &rest[found + pattern.len()..];
            }
        }
        replaced.push_str(rest);
        result = replaced;
    }
    // 组尾残留：`, Symbol }` / `, Symbol;`
    for symbol in dead {
        let tail = format!(", {symbol}");
        let tail_tight = format!(",{symbol}");
        for pattern in [tail, tail_tight] {
            if let Some(found) = result.rfind(&pattern) {
                let after = &result[found + pattern.len()..];
                if after.trim_start().starts_with('}') || after.trim_start().starts_with(';') {
                    let before = &result[..found];
                    let bytes = before.as_bytes();
                    let boundary_ok = before.is_empty()
                        || !(bytes[bytes.len() - 1].is_ascii_alphanumeric()
                            || bytes[bytes.len() - 1] == b'_'
                            || bytes[bytes.len() - 1] == b':');
                    if boundary_ok {
                        result = format!("{}{}", before, after);
                        *removed += 1;
                    }
                }
            }
        }
    }
    result
}

/// 折返改写：crate::Sym → crate::<module>::Sym（限定形态）与
/// use crate::{Sym} → use crate::{module::Sym}（组形态）。
pub fn rewrite_internal_root_consumption(
    repo_root: &Path,
    crate_name: &str,
    report: &FacadeReport,
) -> Result<usize> {
    let mut rewritten = 0;
    let mut internal_files = Vec::new();
    collect_rust_files(
        &repo_root.join(format!("agent/features/{crate_name}/src")),
        &mut internal_files,
    );
    for file in internal_files {
        if file.ends_with("lib.rs") {
            continue;
        }
        let Ok(mut source) = fs::read_to_string(&file) else {
            continue;
        };
        let mut file_changed = false;
        for (symbol, module) in &report.exports {
            if !report.internal_root_consumed.contains(symbol) || module.is_empty() {
                continue;
            }
            let qualified = format!("crate::{symbol}");
            let replacement = format!("crate::{module}::{symbol}");
            let mut cursor = 0;
            while let Some(found) = source[cursor..].find(&qualified) {
                let absolute = cursor + found;
                let end = absolute + qualified.len();
                let bytes = source.as_bytes();
                let before_ok = absolute == 0
                    || !(bytes[absolute - 1].is_ascii_alphanumeric()
                        || bytes[absolute - 1] == b'_');
                if before_ok {
                    source.replace_range(absolute..end, &replacement);
                    rewritten += 1;
                    file_changed = true;
                    cursor = absolute + replacement.len();
                } else {
                    cursor = end;
                }
            }
        }
        if file_changed {
            fs::write(&file, source)?;
        }
    }
    Ok(rewritten)
}

#[cfg(test)]
#[path = "guards_facade_trim_tests.rs"]
mod guards_facade_trim_tests;
