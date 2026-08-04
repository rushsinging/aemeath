//! COLA 分层纯度守卫。
//!
//! 从 `.agents/hooks/check-cola-layer-purity.sh` 的 python heredoc 实现
//! 移植为 Rust（#1500 push 阻断修复：heredoc python 在部分环境 stdin 卡死）。
//! 语义与原实现一致：检查未迁移 feature 的 COLA 分层，并锁定已迁移 feature
//! 的目标目录；Runtime 使用 domain/application/ports/adapters/shared；
//! Context 使用 domain/application/ports/adapters；Storage 使用
//! domain/ports/adapters；Project/Tools/Task 使用 domain/adapters（domain 不得
//! 依赖 adapters）；Audit 仅允许随真实 Usage 交付增量建立的 Hexagonal 层。
//! 例外：少量已登记的迁移期层级倒置（见 `RUNTIME_LAYER_MIGRATION_EXCEPTIONS`）。

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

const FEATURE_LAYERS: [&str; 5] = ["contract", "gateway", "core", "business", "utils"];
const RUNTIME_HEX_LAYERS: [&str; 5] = ["domain", "application", "ports", "adapters", "shared"];
const WORKFLOW_HEX_LAYERS: [&str; 1] = ["domain"];
const PROVIDER_HEX_LAYERS: [&str; 2] = ["domain", "adapters"];
const MEMORY_HEX_LAYERS: [&str; 4] = ["domain", "application", "ports", "adapters"];
const PROVIDER_LEGACY_LAYERS: [&str; 5] = ["api", "business", "contract", "core", "gateway"];
const POLICY_HEX_LAYERS: [&str; 2] = ["domain", "adapters"];
const POLICY_ALLOWED_TOP_LEVEL_FILES: [&str; 3] = ["lib.rs", "domain.rs", "adapters.rs"];
const POLICY_LEGACY_LAYERS: [&str; 6] = [
    "api",
    "business",
    "contract",
    "core",
    "gateway",
    "capabilities",
];
const POLICY_FORBIDDEN_ADAPTER_TYPES: &str =
    r"\b(?:struct|enum)\s+(?:Deny|Approval|RequireApproval)\w*Policy\b";
const STORAGE_HEX_LAYERS: [&str; 3] = ["domain", "ports", "adapters"];
const STORAGE_LEGACY_LAYERS: [&str; 6] = [
    "api",
    "business",
    "contract",
    "gateway",
    "memory_store",
    "task_store",
];
const PROJECT_HEX_LAYERS: [&str; 2] = ["domain", "adapters"];
const PROJECT_ALLOWED_TOP_LEVEL_FILES: [&str; 3] = ["lib.rs", "domain.rs", "adapters.rs"];
const PROJECT_LEGACY_LAYERS: [&str; 6] = [
    "api",
    "business",
    "contract",
    "core",
    "gateway",
    "capabilities",
];
const TOOLS_HEX_LAYERS: [&str; 2] = ["domain", "adapters"];
const TOOLS_ALLOWED_TOP_LEVEL_FILES: [&str; 3] = ["lib.rs", "domain.rs", "adapters.rs"];
const TOOLS_LEGACY_LAYERS: [&str; 5] = ["api", "business", "contract", "core", "gateway"];
const TASK_HEX_LAYERS: [&str; 2] = ["domain", "adapters"];
const TASK_ALLOWED_TOP_LEVEL_FILES: [&str; 3] = ["lib.rs", "domain.rs", "adapters.rs"];
const TASK_LEGACY_LAYERS: [&str; 7] = [
    "api",
    "business",
    "contract",
    "core",
    "gateway",
    "ports",
    "capabilities",
];
const AUDIT_HEX_LAYERS: [&str; 4] = ["domain", "application", "ports", "adapters"];
const AUDIT_ALLOWED_TOP_LEVEL_FILES: [&str; 5] = [
    "lib.rs",
    "domain.rs",
    "application.rs",
    "ports.rs",
    "adapters.rs",
];
const AUDIT_LEGACY_LAYERS: [&str; 6] = [
    "api",
    "business",
    "contract",
    "core",
    "gateway",
    "capabilities",
];
const HOOK_HEX_LAYERS: [&str; 3] = ["domain", "ports", "adapters"];
const HOOK_ALLOWED_TOP_LEVEL_FILES: [&str; 5] = [
    "lib.rs",
    "domain.rs",
    "ports.rs",
    "adapters.rs",
    "capabilities.rs",
];
const HOOK_LEGACY_LAYERS: [&str; 6] = [
    "api",
    "business",
    "contract",
    "core",
    "gateway",
    "capabilities",
];
const CONTEXT_HEX_LAYERS: [&str; 4] = ["domain", "application", "ports", "adapters"];

/// Dependency direction inside a feature: outer/application layers may depend
/// inward; domain/business must not depend on orchestration/gateway/contract,
/// and utils must stay leaf-like.
fn forbidden_layer_deps(current_layer: &str) -> &'static [&'static str] {
    match current_layer {
        "business" => &["core", "gateway", "contract"],
        "utils" => &["business", "core", "gateway", "contract"],
        "contract" => &["business", "core", "gateway", "utils"],
        "gateway" => &["business", "utils"],
        "domain" => &["application", "ports", "adapters"],
        "ports" => &["application", "adapters"],
        "application" => &["adapters"],
        "shared" => &["domain", "application", "ports", "adapters"],
        _ => &[],
    }
}

/// Runtime production code has completed the Hexagonal cutover. Layer
/// inversions are rejected directly; no Runtime migration exceptions remain.
const RUNTIME_LAYER_MIGRATION_EXCEPTIONS: [(&str, &str); 0] = [];

const TOOL_PROFILE_PUBLIC_API: [&str; 3] =
    ["baseline", "derive_restricted", "allowed_capabilities"];

fn use_crate_segment() -> Regex {
    Regex::new(r"\b(?:use\s+)?crate::([A-Za-z_][A-Za-z0-9_]*)").expect("use_crate_segment regex")
}

fn project_domain_adapter_pattern() -> Regex {
    Regex::new(r"\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)")
        .expect("project_domain_adapter regex")
}

fn tool_name_match_pattern() -> Regex {
    Regex::new(concat!(
        r"(?:\bmatch\s+[^{}]*?(?:\btool_?name\b|\.name\b)|",
        r"\bmatches!\s*\([^,]*?(?:\bToolName\b|\btool_?name\b|\.name\b))",
    ))
    .expect("tool_name_match regex")
}

/// Remove comments so architecture vocabulary in documentation is not code.
fn strip_rust_comments(source: &str) -> String {
    let without_block = Regex::new(r"/\*.*?\*/")
        .expect("block comment regex")
        .replace_all(source, "");
    Regex::new(r"//.*")
        .expect("line comment regex")
        .replace_all(&without_block, "")
        .into_owned()
}

/// Return a simple Rust item's brace body; sufficient for source-policy checks.
fn named_block<'a>(source: &'a str, header: &Regex) -> Option<&'a str> {
    let matched = header.find(source)?;
    let opening = source[match_after(matched, source)..].find('{')?;
    let opening = match_after(matched, source) + opening;
    let mut depth = 0usize;
    for (index, ch) in source[opening..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[opening + 1..opening + index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn match_after(matched: regex::Match<'_>, _source: &str) -> usize {
    matched.end()
}

fn tool_profile_violations(source: &str) -> Vec<String> {
    let source = strip_rust_comments(source);
    let mut violations: Vec<String> = Vec::new();
    if let Some(body) = named_block(
        &source,
        &Regex::new(r"\bpub\s+struct\s+ToolProfile\b").unwrap(),
    ) {
        let fields: Vec<&str> =
            Regex::new(r"(?:^|,)\s*(pub(?:\([^)]*\))?\s+)?allowed_capabilities\s*:")
                .unwrap()
                .find_iter(body)
                .map(|m| m.as_str())
                .collect();
        if fields.len() != 1 || fields[0].contains("pub") {
            violations
                .push("ToolProfile.allowed_capabilities must remain a private field".to_string());
        }
    }
    if let Some(body) = named_block(&source, &Regex::new(r"\bimpl\s+ToolProfile\b").unwrap()) {
        let public_methods: HashSet<String> = Regex::new(r"\bpub\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)")
            .unwrap()
            .captures_iter(body)
            .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
            .collect();
        let expansion_api: Vec<&String> = public_methods
            .iter()
            .filter(|method| !TOOL_PROFILE_PUBLIC_API.contains(&method.as_str()))
            .collect();
        if !expansion_api.is_empty() {
            let mut names: Vec<&String> = expansion_api.clone();
            names.sort();
            violations.push(format!(
                "ToolProfile must not expose capability-expanding mutation API: {}",
                names
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if Regex::new(r"\bfn\s+\w+\s*\([^)]*&mut\s+self")
            .unwrap()
            .is_match(body)
        {
            violations.push("ToolProfile must not expose in-place mutation".to_string());
        }
        if Regex::new(concat!(
            r"self\.allowed_capabilities\s*(?:\|=|&=|\^=|=)|",
            r"self\.allowed_capabilities\s*\.\s*(?:insert|extend|union)",
        ))
        .unwrap()
        .is_match(body)
        {
            violations.push("ToolProfile.allowed_capabilities must not be mutated".to_string());
        }
    }
    violations
}

fn tools_authorization_violations(source: &str) -> Vec<String> {
    let source = strip_rust_comments(source);
    let mut violations: Vec<String> = Vec::new();
    let authorization_source =
        Regex::new(r"\b(?:ToolProfile|is_authorized|authoriz\w*|exclud\w*|denylist|blacklist)\b")
            .unwrap()
            .is_match(&source);
    if authorization_source && Regex::new(r"\bexcludes\b").unwrap().is_match(&source) {
        violations
            .push("ToolProfile::excludes/name blacklist authorization is forbidden".to_string());
    }
    if let Some(name_based) = Regex::new(r"\b(?:exclud\w*|denylist|blacklist)\b[\s\S]{0,500}")
        .unwrap()
        .find(&source)
    {
        if tool_name_match_pattern().is_match(name_based.as_str()) {
            violations.push(
                "authorization must not match on ToolName; use declared capabilities".to_string(),
            );
        }
    }
    violations
}

fn tools_boundary_violations(rel_s: &str, source: &str) -> Vec<String> {
    let source = strip_rust_comments(source);
    let mut violations: Vec<String> = Vec::new();
    if rel_s == "agent/features/tools/src/lib.rs"
        && Regex::new(
            r"\bpub\s+(?:use\b[^;]*\b|(?:struct|enum|type)\s+)(?:RegistryScopeBuilder|RegistryScope)\b",
        )
        .unwrap()
        .is_match(&source)
    {
        violations.push(
            "RegistryScopeBuilder/RegistryScope must not enter the tools crate-root facade"
                .to_string(),
        );
    }
    if rel_s.starts_with("agent/features/tools/src/domain")
        && Regex::new(r"\bToolRegistry\b").unwrap().is_match(&source)
    {
        violations.push("ToolRegistry is an adapter and must not enter tools domain".to_string());
    }
    violations
}

fn is_test_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    name.ends_with("_test.rs")
        || name.ends_with("_tests.rs")
        || stem == "tests"
        || path
            .components()
            .any(|component| component.as_os_str().to_str() == Some("tests"))
}

/// Return `(feature, layer)` for feature source files, `None` otherwise.
fn feature_layer_for(root: &Path, path: &Path) -> Option<(String, String)> {
    let features_root = root.join("agent/features");
    let rel = path.strip_prefix(&features_root).ok()?;
    let parts: Vec<&str> = rel
        .components()
        .map(|component| component.as_os_str().to_str().unwrap_or(""))
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let normalized_layer = parts[2].strip_suffix(".rs").unwrap_or(parts[2]);
    if parts[1] == "src" {
        if parts[0] == "runtime" && RUNTIME_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "workflow" && WORKFLOW_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "provider" && PROVIDER_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "memory" && MEMORY_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "context" && CONTEXT_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "policy" && POLICY_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "project" && PROJECT_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "tools" && TOOLS_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "task" && TASK_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "audit" && AUDIT_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "hook" && HOOK_HEX_LAYERS.contains(&normalized_layer) {
            return Some((parts[0].to_string(), normalized_layer.to_string()));
        }
        if parts[0] == "storage" {
            return None;
        }
        if FEATURE_LAYERS.contains(&parts[2]) {
            return Some((parts[0].to_string(), parts[2].to_string()));
        }
    }
    None
}

fn line_layer_violations(current_layer: &str, line: &str) -> Vec<(String, String)> {
    let stripped = line.trim();
    if stripped.is_empty() || stripped.starts_with("//") || stripped.starts_with('*') {
        return Vec::new();
    }
    let mut violations: Vec<(String, String)> = Vec::new();
    for capture in use_crate_segment().captures_iter(line) {
        if let Some(target) = capture.get(1) {
            let target_layer = target.as_str();
            if forbidden_layer_deps(current_layer).contains(&target_layer) {
                violations.push((
                    target_layer.to_string(),
                    format!(
                        "feature layer {current_layer} must not depend on crate::{target_layer}"
                    ),
                ));
            }
        }
    }
    violations
}

fn run_sanity() {
    assert!(
        !line_layer_violations("business", "use crate::core::port::ToolPort;").is_empty(),
        "sanity block failed: business depending on core"
    );
    assert!(
        !line_layer_violations("utils", "let _ = crate::business::Policy::default();").is_empty(),
        "sanity block failed: utils depending on business"
    );
    assert!(
        line_layer_violations("core", "use crate::business::TaskState;").is_empty(),
        "sanity allow failed: core depending on business"
    );
    assert!(
        !line_layer_violations("domain", "use crate::application::Agent;").is_empty(),
        "sanity block failed: runtime domain depending on application"
    );
    assert!(
        !line_layer_violations("application", "use crate::adapters::SdkProjection;").is_empty(),
        "sanity block failed: runtime application depending on adapters"
    );
    assert!(
        line_layer_violations("application", "use crate::domain::Run;").is_empty(),
        "sanity allow failed: runtime application depending on domain"
    );
    assert!(
        line_layer_violations("adapters", "use crate::ports::EventSink;").is_empty(),
        "sanity allow failed: runtime adapter depending on ports"
    );
    assert!(
        line_layer_violations("business", "use crate::utils::normalize_path;").is_empty(),
        "sanity allow failed: business depending on utils"
    );
    assert!(
        !line_layer_violations("domain", "use crate::adapters::git::GitCli;").is_empty(),
        "sanity block failed: Project domain depending on adapters"
    );
    assert!(
        line_layer_violations("adapters", "use crate::domain::git::GitWorktreeOps;").is_empty(),
        "sanity allow failed: Project adapters depending on domain"
    );
    assert!(
        project_domain_adapter_pattern().is_match("use crate::{\n adapters::git::GitCli,\n};"),
        "sanity block failed: multiline braced Project domain dependency"
    );
    assert!(
        project_domain_adapter_pattern()
            .is_match("use crate::{\n domain::types::WorkspaceRead,\n adapters::git::GitCli,\n};"),
        "sanity block failed: non-first braced Project domain dependency"
    );
    assert!(
        project_domain_adapter_pattern().is_match("use crate::\n adapters::git::GitCli;"),
        "sanity block failed: multiline Project domain dependency"
    );
    let safe_profile = "
        pub struct ToolProfile { allowed_capabilities: ToolCapabilities }
        impl ToolProfile {
            pub fn baseline(value: ToolCapabilities) -> Self { Self { allowed_capabilities: value } }
            pub fn derive_restricted(parent: &Self, requested: ToolCapabilities) -> Self {
                Self::baseline(requested & parent.allowed_capabilities)
            }
            pub fn allowed_capabilities(&self) -> ToolCapabilities { self.allowed_capabilities }
        }
    ";
    assert!(
        tool_profile_violations(safe_profile).is_empty(),
        "sanity allow failed: private, shrink-only ToolProfile"
    );
    assert!(
        !tool_profile_violations(
            "pub struct ToolProfile { pub allowed_capabilities: ToolCapabilities }"
        )
        .is_empty(),
        "sanity block failed: public ToolProfile capability field"
    );
    assert!(
        !tool_profile_violations(&safe_profile.replace(
            "pub fn allowed_capabilities",
            "pub fn insert(&mut self, value: ToolCapabilities) { self.allowed_capabilities |= value; } pub fn allowed_capabilities",
        ))
        .is_empty(),
        "sanity block failed: capability-expanding ToolProfile API"
    );
    assert!(
        !tools_authorization_violations(
            "impl ToolProfile { fn excludes(&self, name: &ToolName) -> bool { matches!(name, ToolName::Bash) } }"
        )
        .is_empty(),
        "sanity block failed: ToolProfile name blacklist"
    );
    assert!(
        tools_authorization_violations(
            "fn is_authorized(required: Caps, profile: ToolProfile) -> bool { required.is_subset_of(profile.allowed_capabilities()) }"
        )
        .is_empty(),
        "sanity allow failed: capability authorization"
    );
    assert!(
        !tools_boundary_violations(
            "agent/features/tools/src/lib.rs",
            "pub use domain::RegistryScopeBuilder;"
        )
        .is_empty(),
        "sanity block failed: RegistryScopeBuilder in crate-root facade"
    );
    assert!(
        !tools_boundary_violations(
            "agent/features/tools/src/domain/catalog.rs",
            "use crate::adapters::ToolRegistry;"
        )
        .is_empty(),
        "sanity block failed: ToolRegistry in domain"
    );
    assert!(
        tools_boundary_violations(
            "agent/features/tools/src/adapters/catalog.rs",
            "use super::ToolRegistry;"
        )
        .is_empty(),
        "sanity allow failed: ToolRegistry in adapters"
    );
}

/// 检查仓库 COLA 分层纯度；违规时输出 JSON block 并返回 Err。
pub fn check(root: &Path) -> Result<()> {
    run_sanity();
    let mut violations: Vec<String> = Vec::new();
    let tool_profile_definition_pattern =
        Regex::new(r"\bpub\s+struct\s+ToolProfile\b").expect("tool profile definition regex");
    let storage_domain_adapter_pattern =
        Regex::new(r"\b(?:std|tokio)::fs::|\bPathBuf\b|\bcrate::adapters\b")
            .expect("storage domain adapter regex");
    let mut seen_runtime_exceptions: BTreeSet<(String, String)> = BTreeSet::new();

    let policy_production_adapter = root.join("agent/features/policy/src/adapters.rs");
    if policy_production_adapter.is_file() {
        let policy_adapter = strip_rust_comments(
            &std::fs::read_to_string(&policy_production_adapter)
                .with_context(|| format!("读取 {} 失败", policy_production_adapter.display()))?,
        );
        if Regex::new(POLICY_FORBIDDEN_ADAPTER_TYPES)
            .expect("policy forbidden adapter regex")
            .is_match(&policy_adapter)
        {
            violations.push(
                "agent/features/policy/src/adapters.rs: v0.1.0 production adapter must be AllowAll-only"
                    .to_string(),
            );
        }
    }
    for old_path in ["agent/runtime", "agent/provider", "agent/tools"] {
        let old = root.join(old_path);
        if old.exists() {
            violations.push(format!(
                "{old_path}: runtime/provider/tools must live under agent/features/*"
            ));
        }
    }

    let features_root = root.join("agent/features");
    // 目录分层检查
    let mut src_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&features_root) {
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_str().unwrap_or("").to_string();
                entry.path().join("src").is_dir().then_some(name)
            })
            .collect();
        names.sort();
        for crate_name in names {
            let src = features_root.join(&crate_name).join("src");
            let mut children: Vec<PathBuf> = std::fs::read_dir(&src)
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.path())
                        .collect()
                })
                .unwrap_or_default();
            children.sort();
            for child in children {
                let name = child.file_name().unwrap_or_default().to_str().unwrap_or("");
                if name.starts_with('.') {
                    continue;
                }
                let rel = child
                    .strip_prefix(root)
                    .unwrap_or(&child)
                    .display()
                    .to_string();
                match crate_name.as_str() {
                    "runtime" if child.is_dir() && FEATURE_LAYERS.contains(&name) => {
                        violations.push(format!(
                            "{rel}: Runtime legacy COLA directory is forbidden; use {RUNTIME_HEX_LAYERS:?}"
                        ));
                        continue;
                    }
                    "workflow" => {
                        if child.is_dir() && !WORKFLOW_HEX_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Workflow source directories must be {WORKFLOW_HEX_LAYERS:?}"
                            ));
                        } else if child.is_file() && name != "lib.rs" && name != "domain.rs" {
                            violations.push(format!(
                                "{rel}: Workflow top-level source files must be lib.rs or domain.rs"
                            ));
                        }
                        continue;
                    }
                    "provider" => {
                        if PROVIDER_LEGACY_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Provider legacy fixed layer is forbidden; use domain/ports/adapters"
                            ));
                            continue;
                        }
                        if child.is_dir() && !PROVIDER_HEX_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Provider source directories must be {PROVIDER_HEX_LAYERS:?}"
                            ));
                        }
                        continue;
                    }
                    "policy" => {
                        if POLICY_LEGACY_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Policy legacy fixed layer is forbidden; use {POLICY_HEX_LAYERS:?}"
                            ));
                        } else if child.is_dir() && !POLICY_HEX_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Policy source directories must be {POLICY_HEX_LAYERS:?}"
                            ));
                        } else if child.is_file() && !POLICY_ALLOWED_TOP_LEVEL_FILES.contains(&name)
                        {
                            violations.push(format!(
                                "{rel}: Policy top-level source files must be {POLICY_ALLOWED_TOP_LEVEL_FILES:?}"
                            ));
                        }
                        continue;
                    }
                    "project" => {
                        if PROJECT_LEGACY_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Project legacy fixed layer is forbidden; use {PROJECT_HEX_LAYERS:?}"
                            ));
                        } else if child.is_dir() && !PROJECT_HEX_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Project source directories must be {PROJECT_HEX_LAYERS:?}"
                            ));
                        } else if child.is_file()
                            && !PROJECT_ALLOWED_TOP_LEVEL_FILES.contains(&name)
                        {
                            violations.push(format!(
                                "{rel}: Project top-level source files must be {PROJECT_ALLOWED_TOP_LEVEL_FILES:?}"
                            ));
                        }
                        continue;
                    }
                    "audit" => {
                        if AUDIT_LEGACY_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Audit empty or legacy fixed layer is forbidden; use evidence-backed {AUDIT_HEX_LAYERS:?}"
                            ));
                        } else if child.is_dir() && !AUDIT_HEX_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Audit source directories must be evidence-backed layers {AUDIT_HEX_LAYERS:?}"
                            ));
                        } else if child.is_file() && !AUDIT_ALLOWED_TOP_LEVEL_FILES.contains(&name)
                        {
                            violations.push(format!(
                                "{rel}: Audit top-level source files must be {AUDIT_ALLOWED_TOP_LEVEL_FILES:?}"
                            ));
                        }
                        continue;
                    }
                    "hook" => {
                        if HOOK_LEGACY_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Hook legacy fixed layer is forbidden; use {HOOK_HEX_LAYERS:?}"
                            ));
                        } else if child.is_dir() && !HOOK_HEX_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Hook source directories must be {HOOK_HEX_LAYERS:?}"
                            ));
                        } else if child.is_file() && !HOOK_ALLOWED_TOP_LEVEL_FILES.contains(&name) {
                            violations.push(format!(
                                "{rel}: Hook top-level source files must be {HOOK_ALLOWED_TOP_LEVEL_FILES:?}"
                            ));
                        }
                        continue;
                    }
                    "tools" => {
                        if TOOLS_LEGACY_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: tools legacy fixed layer is forbidden; use {TOOLS_HEX_LAYERS:?}"
                            ));
                        } else if child.is_dir() && !TOOLS_HEX_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: tools source directories must be {TOOLS_HEX_LAYERS:?}"
                            ));
                        } else if child.is_file() && !TOOLS_ALLOWED_TOP_LEVEL_FILES.contains(&name)
                        {
                            violations.push(format!(
                                "{rel}: tools top-level source files must be {TOOLS_ALLOWED_TOP_LEVEL_FILES:?}"
                            ));
                        }
                        continue;
                    }
                    "task" => {
                        if TASK_LEGACY_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Task legacy fixed layer is forbidden; use {TASK_HEX_LAYERS:?}"
                            ));
                        } else if child.is_dir() && !TASK_HEX_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Task source directories must be {TASK_HEX_LAYERS:?}"
                            ));
                        } else if child.is_file() && !TASK_ALLOWED_TOP_LEVEL_FILES.contains(&name) {
                            violations.push(format!(
                                "{rel}: Task top-level source files must be {TASK_ALLOWED_TOP_LEVEL_FILES:?}"
                            ));
                        }
                        continue;
                    }
                    "storage" => {
                        if STORAGE_LEGACY_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Storage legacy fixed layer is forbidden; use {STORAGE_HEX_LAYERS:?}"
                            ));
                        } else if child.is_dir() && !STORAGE_HEX_LAYERS.contains(&name) {
                            violations.push(format!(
                                "{rel}: Storage directory must be a hexagonal layer {STORAGE_HEX_LAYERS:?} or registered transitional module"
                            ));
                        }
                        continue;
                    }
                    "memory" if child.is_dir() && MEMORY_HEX_LAYERS.contains(&name) => {
                        continue;
                    }
                    _ => {}
                }
                if child.is_dir() && !FEATURE_LAYERS.contains(&name) {
                    if crate_name == "runtime" && RUNTIME_HEX_LAYERS.contains(&name) {
                        continue;
                    }
                    if crate_name == "context" && CONTEXT_HEX_LAYERS.contains(&name) {
                        continue;
                    }
                    violations.push(format!(
                        "{rel}: feature src directories must be COLA layers {FEATURE_LAYERS:?}"
                    ));
                }
                src_dirs.push(child);
            }
        }
    }

    // 文件级检查
    let mut rs_files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&features_root, &mut rs_files);
    rs_files.sort();
    for path in rs_files {
        if is_test_path(&path) {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let rel_s = rel.as_posix_path();
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("读取 {} 失败", path.display()))?;
        if rel_s.starts_with("agent/features/tools/src/") {
            for violation in tools_authorization_violations(&source) {
                violations.push(format!("{rel_s}: {violation}"));
            }
            for violation in tools_boundary_violations(&rel_s, &source) {
                violations.push(format!("{rel_s}: {violation}"));
            }
            if tool_profile_definition_pattern.is_match(&strip_rust_comments(&source)) {
                for violation in tool_profile_violations(&source) {
                    violations.push(format!("{rel_s}: {violation}"));
                }
            }
        }
        if (rel_s.starts_with("agent/features/storage/src/domain/")
            || rel_s == "agent/features/storage/src/domain.rs")
            && storage_domain_adapter_pattern.is_match(&source)
        {
            violations.push(format!(
                "{rel_s}: Storage domain must not perform physical I/O, own PathBuf, or depend on adapters"
            ));
        }
        let Some((_feature, layer)) = feature_layer_for(root, &path) else {
            continue;
        };
        if layer == "domain"
            && rel_s.starts_with("agent/features/project/src/")
            && project_domain_adapter_pattern().is_match(&source)
        {
            violations.push(format!(
                "{rel_s}: Project domain must not depend on crate::adapters"
            ));
        }
        for (lineno, line) in source.lines().enumerate() {
            for (target_layer, violation) in line_layer_violations(&layer, line) {
                let exception = (rel_s.clone(), target_layer);
                if RUNTIME_LAYER_MIGRATION_EXCEPTIONS
                    .contains(&(exception.0.as_str(), exception.1.as_str()))
                {
                    seen_runtime_exceptions.insert(exception);
                    continue;
                }
                violations.push(format!(
                    "{rel_s}:{}: {violation}: {}",
                    lineno + 1,
                    line.trim()
                ));
            }
        }
    }

    let stale_runtime: Vec<String> = RUNTIME_LAYER_MIGRATION_EXCEPTIONS
        .iter()
        .filter(|(path, layer)| {
            !seen_runtime_exceptions.contains(&(path.to_string(), layer.to_string()))
        })
        .map(|(path, _)| path.to_string())
        .collect();
    if !stale_runtime.is_empty() {
        violations.push(format!(
            "Runtime hexagonal migration exception list is stale; remove exact path(s): {}",
            stale_runtime.join(", ")
        ));
    }

    if !violations.is_empty() {
        let reason = format!("COLA layer purity guard FAILED:\n{}", violations.join("\n"));
        println!(
            "{}",
            serde_json::json!({ "decision": "block", "reason": reason })
        );
        bail!(reason);
    }
    Ok(())
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

trait AsPosixPath {
    fn as_posix_path(&self) -> String;
}

impl AsPosixPath for Path {
    fn as_posix_path(&self) -> String {
        self.to_string_lossy().replace('\\', "/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanity_checks_pass() {
        run_sanity();
    }

    #[test]
    fn line_layer_violations_block_business_on_core() {
        assert!(!line_layer_violations("business", "use crate::core::port::ToolPort;").is_empty());
    }

    #[test]
    fn feature_layer_for_runtime_hexagon() {
        let root = Path::new(".");
        let path = root.join("agent/features/runtime/src/domain/run.rs");
        assert_eq!(
            feature_layer_for(root, &path),
            Some(("runtime".to_string(), "domain".to_string()))
        );
    }

    #[test]
    fn tool_profile_mutation_api_is_blocked() {
        assert!(!tool_profile_violations(
            "pub struct ToolProfile { pub allowed_capabilities: ToolCapabilities }"
        )
        .is_empty());
    }
}
