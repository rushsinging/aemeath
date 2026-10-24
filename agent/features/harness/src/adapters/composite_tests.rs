//! 组合 registry 契约测试：list 合并与按 handles 路由新组件。

use crate::adapters::composite::CompositeComponentRegistry;
use crate::adapters::skills_registry::SkillsDirectoryRegistry;
use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};

fn skill_id(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::Skill,
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
    std::fs::create_dir_all(scripts_dir.path().join("alpha")).expect("dir");
    std::fs::write(
        scripts_dir.path().join("alpha/SKILL.md"),
        "---\nname: alpha\ndescription: a\n---\n正文\n",
    )
    .expect("write");
    std::fs::write(prose_dir.path().join("overlay.md"), "body").expect("write");

    let scripts_registry: Box<dyn ComponentRegistry> =
        Box::new(SkillsDirectoryRegistry::new(scripts_dir.path()));
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
    assert!(ids.contains(&skill_id("alpha")));
}

/// 新组件（无成员 list 命中）按 handles 路由到 skills registry（#2060：
/// scripts 退役，路由语义以 skills 目录组件验证）。
#[test]
fn new_skill_routed_by_handles() {
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
    let skills_registry: Box<dyn ComponentRegistry> =
        Box::new(SkillsDirectoryRegistry::new(scripts_dir.path()));
    let mut composite = CompositeComponentRegistry::new(vec![prose_registry, skills_registry]);

    let content = ComponentContent {
        frontmatter: None,
        body: "---\nname: fresh\ndescription: 新\n---\n正文".to_string(),
    };
    composite
        .write(&skill_id("fresh"), &content)
        .expect("新 skill 必须路由到 skills registry");
    assert!(scripts_dir.path().join("fresh/SKILL.md").exists());
}
