//! Owner-aware production log target architecture guard.
//!
//! The scanner balances multiline Rust macro delimiters and removes comments,
//! strings, and `cfg(test)` modules before checking production calls.

use super::routing::TargetSpec;
use super::TargetCatalog;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
struct OwnerRule {
    name: &'static str,
    target: &'static str,
    target_expr: &'static str,
}
impl OwnerRule {
    const fn new(name: &'static str, target: &'static str, target_expr: &'static str) -> Self {
        Self {
            name,
            target,
            target_expr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViolationKind {
    BareLogMacro,
    LiteralMacroTarget,
    WrongOwnerTarget,
    UnregisteredConstant,
    WrongOwnerConstant,
    DuplicateOwnerConstant,
    MissingOwnerConstant,
    EmptyOwnerConstant,
    LogMacroAlias,
}
#[derive(Debug)]
struct Violation {
    path: String,
    line: usize,
    kind: ViolationKind,
    detail: String,
}
impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {:?}: {}",
            self.path, self.line, self.kind, self.detail
        )
    }
}

const OWNERS: &[(&str, OwnerRule)] = &[
    (
        "apps/cli/src",
        OwnerRule::new("tui", "aemeath:tui", "crate::LOG_TARGET"),
    ),
    (
        "agent/composition/src",
        OwnerRule::new("composition", "aemeath:composition", "crate::LOG_TARGET"),
    ),
    (
        "agent/shared/src",
        OwnerRule::new("shared", "aemeath:shared", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/audit/src",
        OwnerRule::new("audit", "aemeath:agent:audit", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/config/src",
        OwnerRule::new("config", "", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/context/src",
        OwnerRule::new("context", "aemeath:context", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/hook/src",
        OwnerRule::new("hook", "aemeath:agent:hook", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/memory/src",
        OwnerRule::new("memory", "", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/policy/src",
        OwnerRule::new("policy", "aemeath:agent:policy", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/project/src",
        OwnerRule::new("project", "aemeath:agent:project", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/provider/src",
        OwnerRule::new("provider", "aemeath:agent:provider", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/runtime/src",
        OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/storage/src",
        OwnerRule::new("storage", "aemeath:agent:storage", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/task/src",
        OwnerRule::new("task", "", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/tools/src",
        OwnerRule::new("tools", "aemeath:agent:tools", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/update/src",
        OwnerRule::new("update", "aemeath:agent:update", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/workflow/src",
        OwnerRule::new("workflow", "aemeath:agent:workflow", "crate::LOG_TARGET"),
    ),
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}
fn rust_files_under(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}
fn is_test_path(relative: &str) -> bool {
    relative
        .split('/')
        .any(|part| part == "tests" || part == "test")
        || relative.rsplit('/').next().is_some_and(|name| {
            name.ends_with("_test.rs") || name.ends_with("_tests.rs") || name == "tests.rs"
        })
}

/// Lex source into a same-sized mask: comments and string/char contents become spaces.
fn lexical_mask(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    let mut block = 0usize;
    while i < bytes.len() {
        if block > 0 {
            if i + 1 < bytes.len() && &bytes[i..i + 2] == b"/*" {
                block += 1;
                out[i] = b' ';
                out[i + 1] = b' ';
                i += 2;
                continue;
            }
            if i + 1 < bytes.len() && &bytes[i..i + 2] == b"*/" {
                block -= 1;
                out[i] = b' ';
                out[i + 1] = b' ';
                i += 2;
                continue;
            }
            if bytes[i] != b'\n' {
                out[i] = b' ';
            }
            i += 1;
            continue;
        }
        if i + 1 < bytes.len() && &bytes[i..i + 2] == b"//" {
            while i < bytes.len() && bytes[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if i + 1 < bytes.len() && &bytes[i..i + 2] == b"/*" {
            block = 1;
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
        } else if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    out[i] = b' ';
                    if i + 1 < bytes.len() {
                        out[i + 1] = b' ';
                    }
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    if bytes[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}
fn production_source(source: &str) -> String {
    let mut out = lexical_mask(source).into_bytes();
    let masked = String::from_utf8(out.clone()).unwrap();
    let mut search = 0;
    const CFG_TEST: &str = "#[cfg(test)]";
    while let Some(offset) = masked[search..].find(CFG_TEST) {
        let start = search + offset;
        let mut cursor = start + CFG_TEST.len();
        while masked
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        if !masked[cursor..].starts_with("mod")
            || masked
                .as_bytes()
                .get(cursor + 3)
                .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            search = cursor;
            continue;
        }
        cursor += 3;
        while masked
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        while masked
            .as_bytes()
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            cursor += 1;
        }
        while masked
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        let end = match masked.as_bytes().get(cursor) {
            Some(b';') => cursor + 1,
            Some(b'{') => balanced_end(&masked, cursor, b'{', b'}').unwrap_or(masked.len()),
            _ => {
                search = cursor;
                continue;
            }
        };
        for byte in &mut out[start..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        search = end;
    }
    String::from_utf8(out).unwrap()
}
fn balanced_end(source: &str, open: usize, left: u8, right: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, byte) in source.as_bytes()[open..].iter().enumerate() {
        if *byte == left {
            depth += 1;
        } else if *byte == right {
            depth -= 1;
            if depth == 0 {
                return Some(open + offset + 1);
            }
        }
    }
    None
}
fn line_at(source: &str, offset: usize) -> usize {
    source[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}
fn compact(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

fn contains_identifier(source: &str, identifier: &str) -> bool {
    source.match_indices(identifier).any(|(start, _)| {
        let before = start
            .checked_sub(1)
            .and_then(|index| source.as_bytes().get(index));
        let after = source.as_bytes().get(start + identifier.len());
        let is_ident = |byte: &u8| byte.is_ascii_alphanumeric() || *byte == b'_';
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

fn inspect_source(raw: &str, owner: &OwnerRule, relative: &str) -> Vec<Violation> {
    let source = production_source(raw);
    let mut violations = Vec::new();
    let mut search = 0;
    while let Some(offset) = source[search..].find("use") {
        let start = search + offset;
        let Some(end_rel) = source[start..].find(';') else {
            break;
        };
        let end = start + end_rel + 1;
        let statement = compact(&source[start..end]);
        let starts_at_boundary = start == 0
            || source.as_bytes()[start - 1].is_ascii_whitespace()
            || matches!(source.as_bytes()[start - 1], b'{' | b';');
        if starts_at_boundary
            && statement.starts_with("uselog::")
            && ["trace", "debug", "info", "warn", "error"]
                .iter()
                .any(|level| contains_identifier(&source[start..end], level))
        {
            violations.push(Violation {
                path: relative.into(),
                line: line_at(&source, start),
                kind: ViolationKind::LogMacroAlias,
                detail: "invoke macros as log::level! so ownership remains provable".into(),
            });
        }
        search = end;
    }
    for level in ["trace", "debug", "info", "warn", "error"] {
        let needle = format!("log::{level}!");
        let mut search = 0;
        while let Some(found) = source[search..].find(&needle) {
            let start = search + found;
            let mut open = start + needle.len();
            while source
                .as_bytes()
                .get(open)
                .is_some_and(u8::is_ascii_whitespace)
            {
                open += 1;
            }
            let Some(&delimiter) = source.as_bytes().get(open) else {
                break;
            };
            let close = match delimiter {
                b'(' => b')',
                b'{' => b'}',
                b'[' => b']',
                _ => {
                    search = open + 1;
                    continue;
                }
            };
            let Some(end) = balanced_end(&source, open, delimiter, close) else {
                break;
            };
            // Use raw text for target expression because mask intentionally hides literals.
            let body = raw[open + 1..end - 1].trim_start();
            let line = line_at(&source, start);
            if !body.starts_with("target") || !body[6..].trim_start().starts_with(':') {
                violations.push(Violation {
                    path: relative.into(),
                    line,
                    kind: ViolationKind::BareLogMacro,
                    detail: format!("log::{level}! has no explicit target"),
                });
            } else {
                let expr = body[body.find(':').unwrap() + 1..]
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .trim();
                if expr.starts_with('"') || expr.starts_with("r#") {
                    violations.push(Violation {
                        path: relative.into(),
                        line,
                        kind: ViolationKind::LiteralMacroTarget,
                        detail: "production macro target must use owner LOG_TARGET".into(),
                    });
                } else {
                    let expr = compact(expr);
                    let owner_target = expr == owner.target_expr
                        || expr == "LOG_TARGET"
                        || (relative.starts_with("apps/cli/src") && expr == "$crate::LOG_TARGET");
                    let special = owner.name == "provider"
                        && relative == "agent/features/provider/src/adapters/error_log.rs"
                        && expr == "LLM_API_ERROR_TARGET";
                    if !owner_target && !special {
                        violations.push(Violation {
                            path: relative.into(),
                            line,
                            kind: ViolationKind::WrongOwnerTarget,
                            detail: format!("target {expr:?}, expected {}", owner.target_expr),
                        });
                    }
                }
            }
            search = end;
        }
    }
    for (index, line) in raw.lines().enumerate() {
        if line.contains("const LOG_TARGET") && line.contains('=') {
            let value = line
                .split_once('=')
                .and_then(|(_, rhs)| rhs.split('"').nth(1))
                .unwrap_or("");
            let kind = if TargetCatalog::exact(value).is_none() {
                Some(ViolationKind::UnregisteredConstant)
            } else if value != owner.target {
                Some(ViolationKind::WrongOwnerConstant)
            } else {
                None
            };
            if let Some(kind) = kind {
                violations.push(Violation {
                    path: relative.into(),
                    line: index + 1,
                    kind,
                    detail: format!(
                        "LOG_TARGET {value:?}; owner {} requires {:?}",
                        owner.name, owner.target
                    ),
                });
            }
        }
    }
    violations
}

fn inspect_workspace(root: &Path) -> std::io::Result<Vec<Violation>> {
    let mut violations = Vec::new();
    for (scope, owner) in OWNERS {
        let mut log_calls = 0usize;
        let mut constants = Vec::new();
        for file in rust_files_under(&root.join(scope)) {
            let relative = file
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let raw = fs::read_to_string(file)?;
            let production = production_source(&raw);
            log_calls += ["trace", "debug", "info", "warn", "error"]
                .iter()
                .map(|l| production.matches(&format!("log::{l}!")).count())
                .sum::<usize>();
            if production
                .lines()
                .any(|line| line.contains("const LOG_TARGET"))
            {
                constants.push(relative.clone());
            }
            violations.extend(inspect_source(&raw, owner, &relative));
        }
        if owner.target.is_empty() && !constants.is_empty() {
            violations.push(Violation {
                path: scope.to_string(),
                line: 1,
                kind: ViolationKind::EmptyOwnerConstant,
                detail: format!(
                    "{} has no production logs and must not define LOG_TARGET",
                    owner.name
                ),
            });
        } else if !owner.target.is_empty() {
            // A registered owner must never declare more than one LOG_TARGET,
            // regardless of whether it currently emits production logs.
            if constants.len() > 1 {
                violations.push(Violation {
                    path: scope.to_string(),
                    line: 1,
                    kind: ViolationKind::DuplicateOwnerConstant,
                    detail: constants.join(", "),
                });
            } else if log_calls > 0 && constants.is_empty() {
                violations.push(Violation {
                    path: scope.to_string(),
                    line: 1,
                    kind: ViolationKind::MissingOwnerConstant,
                    detail: format!("{} must define one owner LOG_TARGET", owner.name),
                });
            }
        }
    }
    Ok(violations)
}

#[cfg(test)]
#[path = "routing_guard_tests.rs"]
mod tests;
