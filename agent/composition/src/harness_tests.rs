use std::path::Path;

use harness::domain::component::{ComponentId, ComponentKind, ComponentName};

fn guardrails_id() -> ComponentId {
    ComponentId {
        kind: ComponentKind::Guardrails,
        name: ComponentName::new("default"),
    }
}

fn tool_guidance_id() -> ComponentId {
    ComponentId {
        kind: ComponentKind::ToolGuidance,
        name: ComponentName::new("default"),
    }
}

#[test]
fn builtin_prose_bootstrap_creates_missing_skeletons_atomically_and_idempotently() {
    let root = tempfile::tempdir().expect("tempdir");
    let created = super::ensure_builtin_prose_components_at(root.path()).expect("bootstrap");
    assert_eq!(created, vec![guardrails_id(), tool_guidance_id()]);
    assert_eq!(
        std::fs::read_to_string(root.path().join("guardrails.md")).expect("guardrails"),
        "# Guardrails\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("tool_guidance.md")).expect("tool guidance"),
        "# Tool Guidance\n"
    );

    // 第二次运行不覆盖用户内容。
    std::fs::write(root.path().join("guardrails.md"), "# 用户规则").unwrap();
    let created_again =
        super::ensure_builtin_prose_components_at(root.path()).expect("bootstrap again");
    assert!(created_again.is_empty());
    assert_eq!(
        std::fs::read_to_string(root.path().join("guardrails.md")).unwrap(),
        "# 用户规则"
    );
}

#[test]
fn synthetic_bootstrap_registry_lists_missing_builtin_components_for_dry_run() {
    let root = tempfile::tempdir().expect("tempdir");
    let registry =
        super::wire_component_registry_under(root.path(), super::ComponentBootstrapMode::DryRun)
            .expect("dry registry");
    let guard = registry.try_lock().expect("lock");
    assert!(guard.list().contains(&guardrails_id()));
    assert!(guard.list().contains(&tool_guidance_id()));
    assert!(!root.path().join("guardrails.md").exists());
    assert!(!root.path().join("tool_guidance.md").exists());
}

#[test]
fn session_enumeration_only_accepts_project_scoped_datasets() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(root.path().join("legacy.dataset")).unwrap();
    std::fs::create_dir_all(root.path().join("0123456789abcdef/scoped.dataset")).unwrap();
    std::fs::create_dir_all(root.path().join("not-a-project/other.dataset")).unwrap();
    std::fs::create_dir_all(root.path().join("0123456789abcdef/not-a-dataset")).unwrap();
    let entries = super::enumerate_session_entries(root.path());
    let keys: Vec<_> = entries.into_iter().map(|entry| entry.session_key).collect();
    assert!(!keys.contains(&"legacy.dataset".to_string()));
    assert!(keys.contains(&"0123456789abcdef/scoped.dataset".to_string()));
    assert!(!keys.contains(&"not-a-project/other.dataset".to_string()));
    assert!(!keys.contains(&"0123456789abcdef/not-a-dataset".to_string()));
}

#[test]
fn project_overlay_registry_prefers_agents_md_and_falls_back_to_claude_md() {
    let agents_root = tempfile::tempdir().expect("agents tempdir");
    let project_a = tempfile::tempdir().expect("project a");
    let project_b = tempfile::tempdir().expect("project b");
    let segment_a = "0123456789abcdef";
    let segment_b = "fedcba9876543210";
    let session_dir = agents_root.path().join("session");
    for (segment, root) in [(segment_a, project_a.path()), (segment_b, project_b.path())] {
        std::fs::create_dir_all(session_dir.join(segment)).unwrap();
        let canonical = serde_json::json!({
            "workspace": {
                "state": "captured",
                "value": {
                    "project_identity": {
                        "initial_cwd": root.to_string_lossy(),
                        "git_common_dir": null
                    },
                    "path_base": root.to_string_lossy(),
                    "workspace_root": root.to_string_lossy()
                }
            }
        });
        std::fs::write(
            session_dir
                .join(segment)
                .join("01999999-9999-7999-8999-999999999999"),
            canonical.to_string(),
        )
        .unwrap();
    }
    std::fs::write(project_a.path().join("AGENTS.md"), "# A AGENTS").unwrap();
    std::fs::write(project_a.path().join("CLAUDE.md"), "# A CLAUDE").unwrap();
    std::fs::write(project_b.path().join("CLAUDE.md"), "# B CLAUDE").unwrap();

    let paths = super::project_overlay_component_paths_under(agents_root.path());
    let overlay_a = harness::ComponentId {
        kind: ComponentKind::SystemPromptOverlay,
        name: harness::ComponentName::new(segment_a),
    };
    let overlay_b = harness::ComponentId {
        kind: ComponentKind::SystemPromptOverlay,
        name: harness::ComponentName::new(segment_b),
    };
    assert_eq!(paths[&overlay_a], project_a.path().join("AGENTS.md"));
    assert_eq!(paths[&overlay_b], project_b.path().join("CLAUDE.md"));

    let registry = super::wire_component_registry_under(
        agents_root.path(),
        super::ComponentBootstrapMode::DryRun,
    )
    .expect("registry");
    let registry = registry.try_lock().expect("lock");
    assert!(registry.list().contains(&overlay_a));
    assert!(registry.list().contains(&overlay_b));
    assert_eq!(
        registry
            .read(&overlay_a)
            .expect("read project A")
            .body
            .trim(),
        "# A AGENTS"
    );
    assert_eq!(
        registry
            .read(&overlay_b)
            .expect("read project B")
            .body
            .trim(),
        "# B CLAUDE"
    );
}

#[test]
fn component_path_map_contains_builtin_prose_components() {
    let root = Path::new("/agents");
    let paths = super::builtin_prose_component_paths_under(root);
    let keys: Vec<ComponentId> = paths.keys().cloned().collect();
    assert!(keys.contains(&guardrails_id()));
    assert!(keys.contains(&tool_guidance_id()));
    assert_eq!(paths.len(), 3);
}
