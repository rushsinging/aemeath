//! scripts registry 集成测试：目录枚举 / 读写往返 / 校验拦截 / ChangeSet 集成。

use crate::adapters::scripts_registry::ScriptsDirectoryRegistry;
use crate::application::ComponentRegistry;
use crate::domain::changeset::{apply_change_set, ChangeSet, ComponentEdit};
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};

fn script_id(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::Script,
        name: ComponentName::new(name),
    }
}

/// list 枚举目录脚本（.sh/.py），忽略其他文件。
#[test]
fn list_enumerates_directory_scripts() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("alpha.sh"), "# description: a\n").expect("write");
    std::fs::write(temp.path().join("beta.py"), "# description: b\n").expect("write");
    std::fs::write(temp.path().join("readme.md"), "x").expect("write");

    let registry = ScriptsDirectoryRegistry::new(temp.path());
    let ids = registry.list();
    assert_eq!(ids, vec![script_id("alpha"), script_id("beta")]);
}

/// read 提取 frontmatter（name/description 从注释块）+ body 原文。
#[test]
fn read_extracts_frontmatter_and_body() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("alpha.sh"),
        "#!/bin/bash\n# name: alpha\n# description: 说明\necho ok\n",
    )
    .expect("write");

    let registry = ScriptsDirectoryRegistry::new(temp.path());
    let content = registry.read(&script_id("alpha")).expect("read");
    let frontmatter = content.frontmatter.expect("frontmatter 必须存在");
    assert_eq!(frontmatter["name"], "alpha");
    assert_eq!(frontmatter["description"], "说明");
    assert!(content.body.contains("echo ok"));
}

/// write 新脚本：默认 .sh、目录自动创建、校验先行。
#[test]
fn write_creates_sh_script_with_validation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut registry = ScriptsDirectoryRegistry::new(temp.path().join("scripts"));
    let content = ComponentContent {
        frontmatter: None,
        body: "#!/bin/bash\n# description: 新脚本\necho created\n".to_string(),
    };
    registry
        .write(&script_id("fresh"), &content)
        .expect("write");
    assert!(temp.path().join("scripts/fresh.sh").exists());
    // 校验拦截：语法错误不落盘
    let broken = ComponentContent {
        frontmatter: None,
        body: "# description: 坏\nif true; then\n".to_string(),
    };
    assert!(registry.write(&script_id("broken"), &broken).is_err());
    assert!(!temp.path().join("scripts/broken.sh").exists());
}

/// delete 幂等：存在删除、不存在也算成功。
#[test]
fn delete_is_idempotent() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("alpha.sh"), "# description: a\n").expect("write");
    let mut registry = ScriptsDirectoryRegistry::new(temp.path());
    registry.delete(&script_id("alpha")).expect("delete");
    assert!(!temp.path().join("alpha.sh").exists());
    registry
        .delete(&script_id("alpha"))
        .expect("删除不存在必须幂等");
}

/// ChangeSet 集成：新增脚本经 upsert 原子落盘；校验失败整份作废。
#[test]
fn changeset_upserts_and_rejects_atomically() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut registry = ScriptsDirectoryRegistry::new(temp.path());
    let good = ChangeSet {
        edits: vec![
            ComponentEdit::Upsert {
                id: script_id("good"),
                content: ComponentContent {
                    frontmatter: None,
                    body: "# description: 好脚本\necho ok\n".to_string(),
                },
            },
            ComponentEdit::Upsert {
                id: script_id("bad"),
                content: ComponentContent {
                    frontmatter: None,
                    body: "# description: 坏\nif true; then\n".to_string(),
                },
            },
        ],
    };
    assert!(apply_change_set(&mut registry, &good).is_err());
    assert!(registry.list().is_empty(), "整份作废：一个不落盘");
}
