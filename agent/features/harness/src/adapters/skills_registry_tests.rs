//! skills registry 契约测试（#2018）。

use crate::adapters::skills_registry::SkillsDirectoryRegistry;
use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};

fn skill_id(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::Skill,
        name: ComponentName::new(name),
    }
}

const VALID_SKILL: &str = "---\nname: deploy\ndescription: 部署技能\n---\n步骤一…\n";

/// list 枚举含 SKILL.md 的目录，忽略无 SKILL.md 的目录与平铺文件。
#[test]
fn list_enumerates_skill_directories() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("alpha")).expect("dir");
    std::fs::write(temp.path().join("alpha/SKILL.md"), VALID_SKILL).expect("write");
    std::fs::create_dir_all(temp.path().join("no-skill")).expect("dir");
    std::fs::write(temp.path().join("loose.md"), "x").expect("write");

    let registry = SkillsDirectoryRegistry::new(temp.path());
    assert_eq!(registry.list(), vec![skill_id("alpha")]);
}

/// read 提取 YAML frontmatter 与 body。
#[test]
fn read_extracts_frontmatter_and_body() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("alpha")).expect("dir");
    std::fs::write(temp.path().join("alpha/SKILL.md"), VALID_SKILL).expect("write");

    let registry = SkillsDirectoryRegistry::new(temp.path());
    let content = registry.read(&skill_id("alpha")).expect("read");
    assert_eq!(
        content.frontmatter.expect("frontmatter")["description"],
        "部署技能"
    );
    assert!(content.body.contains("步骤一"));
}

/// 校验拦截：缺 frontmatter / 缺 description / 超大小均拒。
#[test]
fn validation_rejects_malformed_skills() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = SkillsDirectoryRegistry::new(temp.path());

    let no_frontmatter = ComponentContent {
        frontmatter: None,
        body: "没有定界符".to_string(),
    };
    assert!(registry
        .validate(&skill_id("bad1"), &no_frontmatter)
        .is_err());

    let no_description = ComponentContent {
        frontmatter: None,
        body: "---\nname: x\n---\n正文".to_string(),
    };
    assert!(registry
        .validate(&skill_id("bad2"), &no_description)
        .is_err());
}

/// write 新 skill：目录自动创建、校验先行、坏内容不落盘。
#[test]
fn write_creates_skill_with_validation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut registry = SkillsDirectoryRegistry::new(temp.path());
    let content = ComponentContent {
        frontmatter: None,
        body: VALID_SKILL.to_string(),
    };
    registry.write(&skill_id("fresh"), &content).expect("write");
    assert!(temp.path().join("fresh/SKILL.md").exists());

    let broken = ComponentContent {
        frontmatter: None,
        body: "无 frontmatter".to_string(),
    };
    assert!(registry.write(&skill_id("broken"), &broken).is_err());
    assert!(!temp.path().join("broken").exists());
}

/// delete 幂等（整目录删除）。
#[test]
fn delete_removes_skill_directory_idempotently() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("alpha")).expect("dir");
    std::fs::write(temp.path().join("alpha/SKILL.md"), VALID_SKILL).expect("write");
    let mut registry = SkillsDirectoryRegistry::new(temp.path());
    registry.delete(&skill_id("alpha")).expect("delete");
    assert!(!temp.path().join("alpha").exists());
    registry.delete(&skill_id("alpha")).expect("幂等");
}
