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
    LogMacroAlias,
    /// A non-runtime member defines LOG_TARGET, an anonymous keepalive, or
    /// depends directly on logging/log.
    ForbiddenNonRuntimeTarget,
    /// A workspace member that is neither a runtime owner nor a known
    /// non-runtime member.
    UnknownWorkspaceMember,
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
        "apps/cli",
        OwnerRule::new("tui", "aemeath:tui", "crate::LOG_TARGET"),
    ),
    (
        "agent/composition",
        OwnerRule::new("composition", "aemeath:composition", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/audit",
        OwnerRule::new("audit", "aemeath:agent:audit", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/config",
        OwnerRule::new("config", "aemeath:agent:config", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/hook",
        OwnerRule::new("hook", "aemeath:agent:hook", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/memory",
        OwnerRule::new("memory", "aemeath:agent:memory", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/policy",
        OwnerRule::new("policy", "aemeath:agent:policy", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/context",
        OwnerRule::new("context", "aemeath:context", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/project",
        OwnerRule::new("project", "aemeath:agent:project", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/provider",
        OwnerRule::new("provider", "aemeath:agent:provider", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/runtime",
        OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/storage",
        OwnerRule::new("storage", "aemeath:agent:storage", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/task",
        OwnerRule::new("task", "aemeath:agent:task", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/tools",
        OwnerRule::new("tools", "aemeath:agent:tools", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/update",
        OwnerRule::new("update", "aemeath:agent:update", "crate::LOG_TARGET"),
    ),
    (
        "agent/features/workflow",
        OwnerRule::new("workflow", "aemeath:agent:workflow", "crate::LOG_TARGET"),
    ),
    (
        "agent/shared",
        OwnerRule::new("share", "aemeath:shared", "crate::LOG_TARGET"),
    ),
];

/// Workspace members that are intentionally NOT runtime owners: they must not
/// define LOG_TARGET, register a Catalog entry, or directly depend on
/// logging/log. xtask may emit ordinary CLI output but must not apply the
/// logging target architecture.
const NON_RUNTIME_MEMBERS: &[&str] = &[
    "packages/sdk",
    "packages/global/logging",
    "packages/global/utils",
    "tools/xtask",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}
fn workspace_members(root: &Path) -> std::io::Result<Vec<String>> {
    let manifest = match fs::read_to_string(root.join("Cargo.toml")) {
        Ok(manifest) => manifest,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let members = manifest
        .split_once("members")
        .and_then(|(_, rest)| rest.split_once('['))
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(list, _)| {
            list.lines()
                .filter_map(|line| line.split('"').nth(1))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(members)
}

fn crate_root(member: &Path) -> std::io::Result<PathBuf> {
    for name in ["lib.rs", "main.rs"] {
        let root = member.join("src").join(name);
        if root.is_file() {
            return Ok(root);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{} has no crate root", member.display()),
    ))
}

fn owner_constant_declarations(source: &str) -> Vec<String> {
    source
        .lines()
        .filter(|line| {
            line.trim_start().starts_with("pub(crate) const LOG_TARGET") && line.contains('=')
        })
        .filter_map(|line| {
            line.split_once('=')
                .and_then(|(_, rhs)| rhs.split('"').nth(1))
        })
        .map(str::to_owned)
        .collect()
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
    let inspect_constants = relative.ends_with("/lib.rs") || relative.ends_with("/main.rs");
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
        if inspect_constants && line.contains("const LOG_TARGET") && line.contains('=') {
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

/// Path of the guard source file itself; excluded from scans because it
/// legitimately references LOG_TARGET as part of the checking logic.
const GUARD_SELF_PATH: &str = "packages/global/logging/src/domain/routing_guard.rs";
const GUARD_TEST_PATH: &str = "packages/global/logging/src/domain/routing_guard_tests.rs";

/// Check a non-runtime member for forbidden logging-target usage.
fn inspect_non_runtime_member(raw_source: &str, relative: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    // The guard source files legitimately contain LOG_TARGET references.
    if relative == GUARD_SELF_PATH || relative == GUARD_TEST_PATH {
        return violations;
    }
    // Any LOG_TARGET constant declaration is forbidden.
    for (index, line) in raw_source.lines().enumerate() {
        if line.contains("const LOG_TARGET") && line.contains('=') {
            violations.push(Violation {
                path: relative.into(),
                line: index + 1,
                kind: ViolationKind::ForbiddenNonRuntimeTarget,
                detail: "non-runtime member must not define LOG_TARGET".into(),
            });
        }
        // Anonymous keepalive referencing LOG_TARGET.
        if line.contains("const _") && line.contains("LOG_TARGET") {
            violations.push(Violation {
                path: relative.into(),
                line: index + 1,
                kind: ViolationKind::ForbiddenNonRuntimeTarget,
                detail: "non-runtime member must not keepalive LOG_TARGET".into(),
            });
        }
    }
    violations
}

/// Check a non-runtime member's Cargo.toml for a direct logging/log dependency.
///
/// The Logging implementation itself necessarily depends on the `log` facade;
/// the restriction applies to pure contract/function crates and xtask only.
fn inspect_non_runtime_cargo_toml(root: &Path, member: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    if member == "packages/global/logging" {
        return violations;
    }
    let manifest_path = root.join(member).join("Cargo.toml");
    let Ok(manifest) = fs::read_to_string(&manifest_path) else {
        return violations;
    };
    // Parse the [dependencies] table lines for a direct `log` or `logging` dependency.
    // A workspace-inherited dependency line looks like `log.workspace = true`.
    let in_deps = |section: &str| section.trim() == "[dependencies]";
    let mut section = String::new();
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.to_string();
            continue;
        }
        if !in_deps(&section) {
            continue;
        }
        // Dependency lines look like `name = ...` or `name.workspace = true`.
        let dep_name = trimmed
            .split_once('=')
            .map(|(lhs, _)| lhs.trim())
            .unwrap_or("");
        if dep_name == "log" || dep_name == "logging" {
            violations.push(Violation {
                path: format!("{member}/Cargo.toml"),
                line: 1,
                kind: ViolationKind::ForbiddenNonRuntimeTarget,
                detail: format!("non-runtime member must not directly depend on {dep_name}"),
            });
        }
    }
    violations
}

fn inspect_workspace(root: &Path) -> std::io::Result<Vec<Violation>> {
    let mut violations = Vec::new();
    let members = workspace_members(root)?;
    for member in &members {
        let is_owner = OWNERS.iter().any(|(registered, _)| registered == member);
        let is_non_runtime = NON_RUNTIME_MEMBERS.iter().any(|nr| *nr == member);
        if !is_owner && !is_non_runtime {
            violations.push(Violation {
                path: member.clone(),
                line: 1,
                kind: ViolationKind::UnknownWorkspaceMember,
                detail:
                    "workspace member is neither a runtime owner nor a known non-runtime member"
                        .into(),
            });
        }
    }
    for (member, owner) in OWNERS {
        if !members.is_empty()
            && !members
                .iter()
                .any(|workspace_member| workspace_member == member)
        {
            violations.push(Violation {
                path: (*member).into(),
                line: 1,
                kind: ViolationKind::MissingOwnerConstant,
                detail: "owner rule is not a workspace member".into(),
            });
            continue;
        }
        let scope = format!("{member}/src");
        let mut constants = Vec::new();
        for file in rust_files_under(&root.join(&scope)) {
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
            for _ in production.lines().filter(|line| {
                relative != "packages/global/logging/src/domain/routing_guard.rs"
                    && line.contains("const LOG_TARGET")
            }) {
                constants.push(relative.clone());
            }
            violations.extend(inspect_source(&raw, owner, &relative));
        }
        let root_path = crate_root(&root.join(member));
        let root_declarations = root_path
            .as_ref()
            .ok()
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|source| owner_constant_declarations(&source))
            .unwrap_or_default();
        if constants.len() > 1 || root_declarations.len() > 1 {
            violations.push(Violation {
                path: scope,
                line: 1,
                kind: ViolationKind::DuplicateOwnerConstant,
                detail: constants.join(", "),
            });
        } else if constants.len() != 1 || root_declarations != [owner.target.to_owned()] {
            violations.push(Violation {
                path: member.to_string(),
                line: 1,
                kind: ViolationKind::MissingOwnerConstant,
                detail: format!(
                    "{} crate root must define exactly one pub(crate) LOG_TARGET {:?}",
                    owner.name, owner.target
                ),
            });
        }
    }
    // Inspect non-runtime members for forbidden logging usage.
    for member in NON_RUNTIME_MEMBERS {
        if !members.is_empty()
            && !members
                .iter()
                .any(|workspace_member| workspace_member == member)
        {
            // Non-runtime member absent from this workspace — nothing to check.
            continue;
        }
        let scope = format!("{member}/src");
        for file in rust_files_under(&root.join(&scope)) {
            let relative = file
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let raw = fs::read_to_string(file)?;
            violations.extend(inspect_non_runtime_member(&raw, &relative));
        }
        violations.extend(inspect_non_runtime_cargo_toml(root, member));
    }
    Ok(violations)
}

#[cfg(test)]
#[path = "routing_guard_tests.rs"]
mod tests;
