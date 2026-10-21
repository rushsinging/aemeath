//! 文件 adapter 契约测试：frontmatter 往返、原子写、校验拦截、删除语义。

use crate::adapters::filesystem::FilesystemComponentRegistry;
use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use std::collections::HashMap;

fn id(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::Skill,
        name: ComponentName::new(name),
    }
}

fn registry_under(root: &std::path::Path, names: &[&str]) -> FilesystemComponentRegistry {
    let mut component_paths = HashMap::new();
    for name in names {
        component_paths.insert(id(name), root.join(format!("{name}.md")));
    }
    FilesystemComponentRegistry::new(component_paths)
}

/// frontmatter + 正文写入后读回等价（round-trip）。
#[test]
fn frontmatter_and_body_survive_round_trip() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut registry = registry_under(temp_dir.path(), &["alpha"]);
    let content = ComponentContent {
        frontmatter: Some(serde_json::json!({"name": "alpha", "description": "示例"})),
        body: "# 正文\n\n内容".to_string(),
    };

    registry.write(&id("alpha"), &content).expect("写入成功");
    let read_back = registry.read(&id("alpha")).expect("读回成功");

    assert_eq!(read_back, content);
}

/// 无 frontmatter 的纯正文组件同样可往返。
#[test]
fn body_only_content_round_trips() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut registry = registry_under(temp_dir.path(), &["plain"]);
    let content = ComponentContent {
        frontmatter: None,
        body: "纯正文".to_string(),
    };

    registry.write(&id("plain"), &content).expect("写入成功");
    assert_eq!(registry.read(&id("plain")).expect("读回成功"), content);
}

/// 校验失败（frontmatter 非 object）NEVER 落盘：文件不出现，tmp 不残留。
#[test]
fn invalid_frontmatter_never_reaches_disk() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut registry = registry_under(temp_dir.path(), &["bad"]);
    let content = ComponentContent {
        frontmatter: Some(serde_json::json!([1, 2])),
        body: "正文".to_string(),
    };

    let error = registry
        .write(&id("bad"), &content)
        .expect_err("frontmatter 非映射必须被校验拒绝");

    assert!(error.message().contains("frontmatter"));
    assert!(
        !temp_dir.path().join("bad.md").exists(),
        "校验失败 NEVER 落盘"
    );
    assert!(
        !temp_dir.path().join("bad.tmp").exists(),
        "NEVER 残留临时文件"
    );
}

/// 原子写契约：写入目录不存在时自动创建父目录并成功落盘。
#[test]
fn atomic_write_creates_missing_parent_directories() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let nested = temp_dir.path().join("skills/nested");
    let mut component_paths = HashMap::new();
    component_paths.insert(id("deep"), nested.join("deep.md"));
    let mut registry = FilesystemComponentRegistry::new(component_paths);

    let content = ComponentContent {
        frontmatter: None,
        body: "深层组件".to_string(),
    };
    registry.write(&id("deep"), &content).expect("写入成功");
    assert!(nested.join("deep.md").exists());
}

/// 删除语义：移除文件并从 list 消失；重复删除幂等成功。
#[test]
fn delete_removes_file_and_is_idempotent() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut registry = registry_under(temp_dir.path(), &["doomed"]);
    let content = ComponentContent {
        frontmatter: None,
        body: "待删除".to_string(),
    };
    registry.write(&id("doomed"), &content).expect("写入成功");

    registry.delete(&id("doomed")).expect("删除成功");
    registry.delete(&id("doomed")).expect("重复删除幂等成功");

    assert!(registry.list().is_empty());
    assert!(!temp_dir.path().join("doomed.md").exists());
}

/// 未注册路径的组件：read/write/delete 均 Invalid。
#[test]
fn unregistered_component_rejected_on_all_operations() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut registry = registry_under(temp_dir.path(), &["known"]);
    let ghost = ComponentId {
        kind: ComponentKind::Plugin,
        name: ComponentName::new("ghost"),
    };
    let content = ComponentContent::default();

    assert!(registry.read(&ghost).is_err());
    assert!(registry.write(&ghost, &content).is_err());
    assert!(registry.delete(&ghost).is_err());
    // list 语义 = 文件在盘的组件：known 尚未写入，list 为空；写入后出现。
    assert!(registry.list().is_empty());
    registry
        .write(&id("known"), &content)
        .expect("写入已注册组件");
    assert_eq!(registry.list(), vec![id("known")]);
}
