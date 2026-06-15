//! PolicyEngine — centralized permission gate for all tool calls.
//!
//! Absorbs the former `split_approved_calls` + scattered `path_security`
//! checks into a single evaluation point that runs **before** any tool
//! executes.

use std::path::{Path, PathBuf};

use serde_json::Value;
use share::tool::{PathAccess, PathKind, PolicyDecision};
use tools::api::Tool;

use policy::api::{
    validate_and_normalize_path_from_base, validate_search_path_from_base,
};

/// The unified permission gate.
///
/// Holds the context needed to evaluate tool calls: the active `path_base`
/// (for resolving relative paths), the `workspace_root` (security boundary),
/// and the `allow_all` flag (AllowAll mode bypasses workspace boundary
/// checks for all tools).
pub struct PolicyEngine<'a> {
    path_base: &'a Path,
    workspace_root: &'a Path,
    allow_all: bool,
}

/// A tool call that was denied by the engine, with the reason.
#[derive(Debug, Clone)]
pub struct DeniedCall {
    pub id: String,
    pub name: String,
    pub reason: String,
}

impl<'a> PolicyEngine<'a> {
    pub fn new(path_base: &'a Path, workspace_root: &'a Path, allow_all: bool) -> Self {
        Self {
            path_base,
            workspace_root,
            allow_all,
        }
    }

    /// Evaluate a single tool call's input against policy.
    ///
    /// 1. **Tool-level**: AllowAll / read-only / input-safety check
    ///    (absorbs `is_auto_approved`).
    /// 2. **Resource-level**: path boundary validation + normalisation
    ///    for every declared `PathAccess`.
    ///
    /// If allowed, the returned `PolicyDecision::Allow` contains the input
    /// with all path fields normalised to absolute paths.
    pub fn evaluate(&self, input: &Value, tool: Option<&dyn Tool>) -> PolicyDecision {
        // Step 1: tool-level auto-approval
        let auto_approved = match tool {
            Some(t) => self.allow_all || t.is_read_only() || t.is_input_safe(input),
            None => self.allow_all,
        };

        if !auto_approved {
            return PolicyDecision::Deny {
                reason: "This tool requires user confirmation.".into(),
            };
        }

        // Step 2: resource-level path validation + normalisation
        let Some(t) = tool else {
            return PolicyDecision::Allow(input.clone());
        };

        let accesses: &[PathAccess] = t.path_accesses();
        if accesses.is_empty() {
            return PolicyDecision::Allow(input.clone());
        }

        let mut normalized = input.clone();
        for access in accesses {
            let Some(raw) = normalized.get(access.field).and_then(|v| v.as_str()) else {
                continue;
            };

            let result: Result<PathBuf, String> = match access.kind {
                PathKind::File => validate_and_normalize_path_from_base(
                    raw,
                    self.path_base,
                    self.workspace_root,
                    self.allow_all,
                ),
                PathKind::SearchDir => validate_search_path_from_base(
                    raw,
                    self.path_base,
                    self.workspace_root,
                    self.allow_all,
                ),
            };

            match result {
                Ok(path) => {
                    normalized[access.field] = Value::String(path.to_string_lossy().into_owned());
                }
                Err(reason) => {
                    return PolicyDecision::Deny { reason };
                }
            }
        }

        PolicyDecision::Allow(normalized)
    }
}
