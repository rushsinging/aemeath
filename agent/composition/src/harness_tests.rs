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
fn session_enumeration_includes_project_scoped_datasets() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(root.path().join("legacy.dataset")).unwrap();
    std::fs::create_dir_all(root.path().join("project-a/scoped.dataset")).unwrap();
    std::fs::create_dir_all(root.path().join("project-a/not-a-dataset")).unwrap();
    let entries = super::enumerate_session_entries(root.path());
    let keys: Vec<_> = entries.into_iter().map(|entry| entry.session_key).collect();
    assert!(keys.contains(&"legacy.dataset".to_string()));
    assert!(keys.contains(&"project-a/scoped.dataset".to_string()));
    assert!(!keys.contains(&"project-a/not-a-dataset".to_string()));
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
