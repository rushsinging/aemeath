//! ChangeSet 编辑契约测试：omitted-is-inherited 与整份作废语义。

use crate::application::ComponentRegistry;
use crate::domain::changeset::{apply_change_set, AppliedEdit, ChangeSet, ComponentEdit};
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::test_support::FakeRegistry;

fn id(kind: ComponentKind, name: &str) -> ComponentId {
    ComponentId {
        kind,
        name: ComponentName::new(name),
    }
}

fn body_content(body: &str) -> ComponentContent {
    ComponentContent {
        frontmatter: None,
        body: body.to_string(),
    }
}

/// omitted-is-inherited：提案未提及的组件在 apply 后保持原内容。
#[test]
fn omitted_components_are_inherited_unchanged() {
    let mentioned = id(ComponentKind::Skill, "mentioned");
    let untouched = id(ComponentKind::Guardrails, "untouched");
    let mut registry = FakeRegistry::with_entries(vec![
        (mentioned.clone(), body_content("old")),
        (untouched.clone(), body_content("keep-me")),
    ]);

    let change_set = ChangeSet {
        edits: vec![ComponentEdit::Upsert {
            id: mentioned.clone(),
            content: body_content("new"),
        }],
    };
    let applied = apply_change_set(&mut registry, &change_set).expect("apply 成功");

    assert_eq!(
        applied,
        vec![AppliedEdit::Upserted {
            id: mentioned.clone()
        }],
    );
    assert_eq!(
        registry.content_of(&mentioned).map(|content| &content.body),
        Some(&"new".to_string()),
    );
    assert_eq!(
        registry.content_of(&untouched).map(|content| &content.body),
        Some(&"keep-me".to_string()),
    );
}

/// 任一条 edit 校验失败 → 整份 ChangeSet 作废：合法条目也 NEVER 写入。
#[test]
fn any_invalid_edit_discards_whole_change_set() {
    let good = id(ComponentKind::Skill, "good");
    let bad = id(ComponentKind::Skill, "bad");
    let mut registry = FakeRegistry::with_entries(vec![
        (good.clone(), body_content("old-good")),
        (bad.clone(), body_content("old-bad")),
    ]);
    registry.invalid_ids.push(bad.clone());

    let change_set = ChangeSet {
        edits: vec![
            ComponentEdit::Upsert {
                id: good.clone(),
                content: body_content("new-good"),
            },
            ComponentEdit::Upsert {
                id: bad.clone(),
                content: body_content("new-bad"),
            },
        ],
    };
    let error = apply_change_set(&mut registry, &change_set).expect_err("含非法条目必须整体作废");

    assert!(
        error.message().contains("bad"),
        "错误信息应定位失败条目：{error}"
    );
    assert!(
        registry.writes.is_empty(),
        "整份作废时 NEVER 发生任何写入：{:?}",
        registry.writes,
    );
    assert_eq!(
        registry.content_of(&good).map(|content| &content.body),
        Some(&"old-good".to_string()),
    );
}

/// Delete 不存在的组件 → 整份 ChangeSet 作废（含同批合法条目）。
#[test]
fn delete_missing_component_discards_whole_change_set() {
    let existing = id(ComponentKind::Script, "existing");
    let ghost = id(ComponentKind::Script, "ghost");
    let mut registry = FakeRegistry::with_entries(vec![(existing.clone(), body_content("old"))]);

    let change_set = ChangeSet {
        edits: vec![
            ComponentEdit::Upsert {
                id: existing.clone(),
                content: body_content("new"),
            },
            ComponentEdit::Delete { id: ghost.clone() },
        ],
    };
    let error = apply_change_set(&mut registry, &change_set).expect_err("删除不存在的组件必须作废");

    assert!(
        error.message().contains("ghost"),
        "错误信息应定位失败条目：{error}"
    );
    assert!(registry.writes.is_empty() && registry.deletes.is_empty());
    assert_eq!(
        registry.content_of(&existing).map(|content| &content.body),
        Some(&"old".to_string()),
    );
}

/// 全部合法时按输入顺序统一应用，返回应用清单。
#[test]
fn successful_apply_returns_edits_in_order() {
    let first = id(ComponentKind::Skill, "first");
    let second = id(ComponentKind::Skill, "second");
    let mut registry = FakeRegistry::with_entries(vec![(second.clone(), body_content("old"))]);

    let change_set = ChangeSet {
        edits: vec![
            ComponentEdit::Upsert {
                id: first.clone(),
                content: body_content("created"),
            },
            ComponentEdit::Delete { id: second.clone() },
        ],
    };
    let applied = apply_change_set(&mut registry, &change_set).expect("apply 成功");

    assert_eq!(
        applied,
        vec![
            AppliedEdit::Upserted { id: first },
            AppliedEdit::Deleted { id: second },
        ],
    );
    assert_eq!(registry.list().len(), 1);
}
