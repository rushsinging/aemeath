//! PolicyEngine — centralized permission gate for all tool calls.
//!
//! Absorbs the former `split_approved_calls` + scattered `path_security`
//! checks into a single evaluation point that runs **before** any tool
//! executes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;
use share::tool::{PathAccess, PathKind, PolicyDecision};
use tools::api::Tool;

use policy::api::{validate_and_normalize_path_from_base, validate_search_path_from_base};

/// The unified permission gate.
///
/// Holds the context needed to evaluate tool calls: the active `path_base`
/// (for resolving relative paths), the `workspace_root` (security boundary),
/// the `allow_all` flag (AllowAll mode bypasses workspace boundary
/// checks for all tools), and the session-scoped `read_files` set used by the
/// read-before-write policy.
pub struct PolicyEngine<'a> {
    path_base: &'a Path,
    workspace_root: &'a Path,
    allow_all: bool,
    read_files: &'a Mutex<HashSet<String>>,
}

/// A tool call that was denied by the engine, with the reason.
#[derive(Debug, Clone)]
pub struct DeniedCall {
    pub id: String,
    pub name: String,
    pub reason: String,
}

impl<'a> PolicyEngine<'a> {
    pub fn new(
        path_base: &'a Path,
        workspace_root: &'a Path,
        allow_all: bool,
        read_files: &'a Mutex<HashSet<String>>,
    ) -> Self {
        Self {
            path_base,
            workspace_root,
            allow_all,
            read_files,
        }
    }

    /// Evaluate a single tool call's input against policy.
    ///
    /// 1. **Tool-level**: AllowAll / read-only / input-safety check
    ///    (absorbs `is_auto_approved`).
    /// 2. **Resource-level**: path boundary validation + normalisation
    ///    for every declared `PathAccess`.
    /// 3. **State-level**: read-before-write — if the tool declares
    ///    `requires_read_before_write`, any `PathKind::File` access that
    ///    resolves to an existing file must already be recorded in the
    ///    session's `read_files` set, otherwise the call is denied.
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

        let enforce_read_before_write = t.requires_read_before_write();
        let read_files_snapshot: HashSet<String> = if enforce_read_before_write {
            self.read_files
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_default()
        } else {
            HashSet::new()
        };

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
                    // Step 3: read-before-write check (only for file accesses on
                    // tools that declare the requirement). New (non-existent)
                    // files are always allowed; only existing files require a
                    // prior Read. Both the raw input string and the normalised
                    // path are accepted as evidence of a prior Read, so the
                    // check stays robust regardless of which form the Read tool
                    // recorded.
                    if enforce_read_before_write
                        && access.kind == PathKind::File
                        && path.exists()
                        && !read_files_snapshot.contains(raw)
                        && !read_files_snapshot.contains(path.to_string_lossy().as_ref())
                    {
                        return PolicyDecision::Deny {
                            reason: format!(
                                "You must read {} before editing it. Use the Read tool first.",
                                path.display()
                            ),
                        };
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// Stub tool that declares a single file access and the
    /// read-before-write requirement, mirroring Edit/Write behaviour without
    /// pulling in their heavy `call` implementation.
    struct StubWriteTool;
    #[async_trait::async_trait]
    impl Tool for StubWriteTool {
        fn name(&self) -> &str {
            "StubWrite"
        }
        fn description(&self) -> &str {
            ""
        }
        fn input_schema(&self) -> Value {
            json!({})
        }
        fn is_read_only(&self) -> bool {
            true
        }
        fn path_accesses(&self) -> &'static [PathAccess] {
            &FILE_ACCESS
        }
        fn requires_read_before_write(&self) -> bool {
            true
        }
        async fn call(
            &self,
            _: Value,
            _: &tools::api::ToolExecutionContext,
        ) -> share::tool::ToolResult {
            unreachable!()
        }
    }

    const FILE_ACCESS: [PathAccess; 1] = [PathAccess {
        field: "file_path",
        kind: PathKind::File,
    }];

    fn engine<'e>(base: &'e Path, read_files: &'e Mutex<HashSet<String>>) -> PolicyEngine<'e> {
        PolicyEngine::new(base, base, true, read_files)
    }

    #[test]
    fn test_read_before_write_denies_edit_on_unread_existing_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("existing.txt");
        std::fs::write(&file, "content").unwrap();
        let read_files = Mutex::new(HashSet::new());

        let decision = engine(dir.path(), &read_files).evaluate(
            &json!({ "file_path": file.to_string_lossy() }),
            Some(&StubWriteTool),
        );

        match decision {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("must read"), "got: {reason}");
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn test_read_before_write_allows_edit_on_read_existing_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("existing.txt");
        std::fs::write(&file, "content").unwrap();
        let read_files = Mutex::new(HashSet::from([file.to_string_lossy().to_string()]));

        let decision = engine(dir.path(), &read_files).evaluate(
            &json!({ "file_path": file.to_string_lossy() }),
            Some(&StubWriteTool),
        );

        assert!(matches!(decision, PolicyDecision::Allow(_)));
    }

    #[test]
    fn test_read_before_write_allows_write_new_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("new.txt");
        let read_files = Mutex::new(HashSet::new());

        let decision = engine(dir.path(), &read_files).evaluate(
            &json!({ "file_path": file.to_string_lossy() }),
            Some(&StubWriteTool),
        );

        assert!(matches!(decision, PolicyDecision::Allow(_)));
    }

    #[test]
    fn test_read_before_write_denies_overwrite_existing_unread_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("existing.txt");
        std::fs::write(&file, "content").unwrap();
        let read_files = Mutex::new(HashSet::new());

        let decision = engine(dir.path(), &read_files).evaluate(
            &json!({ "file_path": file.to_string_lossy() }),
            Some(&StubWriteTool),
        );

        assert!(matches!(decision, PolicyDecision::Deny { .. }));
    }
}
