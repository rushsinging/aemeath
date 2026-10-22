//! 组合 registry 契约测试：list 合并与按 handles 路由新组件。

use crate::adapters::composite::CompositeComponentRegistry;
use crate::adapters::scripts_registry::ScriptsDirectoryRegistry;
use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};

fn script_id(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::Script,
        name: ComponentName::new(name),
    }
}

fn overlay_id() -> ComponentId {
    ComponentId {
        kind: ComponentKind::SystemPromptOverlay,
        name: ComponentName::new("overlay"),
    }
}

/// list 合并成员并去重排序。
#[test]
fn list_merges_members() {
    let scripts_dir = tempfile::tempdir().expect("tempdir");
    let prose_dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(scripts_dir.path().join("alpha.sh"), "# description: a\n").expect("write");
    std::fs::write(prose_dir.path().join("overlay.md"), "body").expect("write");

    let scripts_registry: Box<dyn ComponentRegistry> =
        Box::new(ScriptsDirectoryRegistry::new(scripts_dir.path()));
    let prose_registry: Box<dyn ComponentRegistry> = {
        use crate::adapters::filesystem::FilesystemComponentRegistry;
        Box::new(FilesystemComponentRegistry::new(
            [(overlay_id(), prose_dir.path().join("overlay.md"))]
                .into_iter()
                .collect(),
        ))
    };
    let composite = CompositeComponentRegistry::new(vec![prose_registry, scripts_registry]);
    let ids = composite.list();
    assert!(ids.contains(&overlay_id()));
    assert!(ids.contains(&script_id("alpha")));
}

/// 新组件（无成员 list 命中）按 handles 路由到 scripts registry。
#[test]
fn new_script_routed_by_handles() {
    let scripts_dir = tempfile::tempdir().expect("tempdir");
    let prose_dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(prose_dir.path().join("overlay.md"), "body").expect("write");

    let prose_registry: Box<dyn ComponentRegistry> = {
        use crate::adapters::filesystem::FilesystemComponentRegistry;
        Box::new(FilesystemComponentRegistry::new(
            [(overlay_id(), prose_dir.path().join("overlay.md"))]
                .into_iter()
                .collect(),
        ))
    };
    let scripts_registry: Box<dyn ComponentRegistry> =
        Box::new(ScriptsDirectoryRegistry::new(scripts_dir.path()));
    let mut composite = CompositeComponentRegistry::new(vec![prose_registry, scripts_registry]);

    let content = ComponentContent {
        frontmatter: None,
        body: "# description: 新\necho ok\n".to_string(),
    };
    composite
        .write(&script_id("fresh"), &content)
        .expect("新脚本必须路由到 scripts registry");
    assert!(scripts_dir.path().join("fresh.sh").exists());
}
