//! ChangeSet 编辑契约测试：omitted-is-inherited 与整份作废语义。

use crate::application::ComponentRegistry;
use crate::domain::changeset::{
    apply_change_set, apply_change_set_with_policy, AppliedEdit, ChangeSet, ComponentEdit,
    PatchOperation, WritePolicy,
};
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
    let existing = id(ComponentKind::Skill, "existing");
    let ghost = id(ComponentKind::Skill, "ghost");
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

/// Patch 只替换唯一锚点，保留其余正文。
#[test]
fn patch_replaces_unique_anchor_and_preserves_rest() {
    let target = id(ComponentKind::Skill, "progressive");
    let mut registry = FakeRegistry::with_entries(vec![(
        target.clone(),
        body_content("## A\nold\n## B\nkeep\n"),
    )]);

    let change_set = ChangeSet {
        edits: vec![ComponentEdit::Patch {
            id: target.clone(),
            operations: vec![
                PatchOperation::ReplaceExact {
                    old_string: "old".to_string(),
                    new_string: "new".to_string(),
                },
                PatchOperation::InsertAfterAnchor {
                    anchor: "## B\n".to_string(),
                    new_text: "- appended\n".to_string(),
                },
            ],
        }],
    };
    let applied = apply_change_set(&mut registry, &change_set).expect("patch apply");

    assert_eq!(applied, vec![AppliedEdit::Patched { id: target.clone() }]);
    assert_eq!(
        registry.content_of(&target).map(|content| &content.body),
        Some(&"## A\nnew\n## B\n- appended\nkeep\n".to_string()),
    );
}

/// 非唯一 old_string 必须在写前拒绝，正文保持不变。
#[test]
fn patch_non_unique_anchor_is_rejected_before_write() {
    let target = id(ComponentKind::Skill, "progressive");
    let mut registry =
        FakeRegistry::with_entries(vec![(target.clone(), body_content("same\nsame\n"))]);

    let change_set = ChangeSet {
        edits: vec![ComponentEdit::Patch {
            id: target.clone(),
            operations: vec![PatchOperation::ReplaceExact {
                old_string: "same".to_string(),
                new_string: "new".to_string(),
            }],
        }],
    };
    let error = apply_change_set(&mut registry, &change_set).expect_err("非唯一锚点必须拒绝");

    assert!(error.message().contains("唯一"), "{error}");
    assert!(registry.writes.is_empty());
    assert_eq!(
        registry.content_of(&target).map(|content| &content.body),
        Some(&"same\nsame\n".to_string()),
    );
}

/// 多条目中途写入失败时按写前快照回滚已写条目。
#[test]
fn apply_rolls_back_written_edits_when_later_write_fails() {
    let first = id(ComponentKind::Skill, "first");
    let second = id(ComponentKind::Skill, "second");
    let mut registry = FakeRegistry::with_entries(vec![(second.clone(), body_content("old"))]);
    registry.fail_write_ids.push(second.clone());

    let change_set = ChangeSet {
        edits: vec![
            ComponentEdit::Upsert {
                id: first.clone(),
                content: body_content("created"),
            },
            ComponentEdit::Upsert {
                id: second.clone(),
                content: body_content("new"),
            },
        ],
    };
    let error = apply_change_set(&mut registry, &change_set).expect_err("中途失败必须报错");

    assert!(error.message().contains("模拟写入失败"), "{error}");
    assert_eq!(registry.content_of(&first), None, "新建文件必须回滚删除");
    assert_eq!(
        registry.content_of(&second).map(|content| &content.body),
        Some(&"old".to_string()),
        "已有组件必须恢复旧内容"
    );
}

#[test]
fn evidence_gate_rejects_duplicate_rule_increase() {
    let id = id(ComponentKind::Guardrails, "default");
    let mut registry = FakeRegistry::with_entries(vec![(
        id.clone(),
        body_content("# Guardrails\n\n- alpha rule\n- beta rule\n"),
    )]);
    let change_set = ChangeSet {
        edits: vec![ComponentEdit::Upsert {
            id: id.clone(),
            content: body_content("# Guardrails\n\n- alpha rule\n- alpha rule\n"),
        }],
    };
    let error =
        apply_change_set_with_policy(&mut registry, &change_set, WritePolicy::ProseEvidence)
            .expect_err("重复规则增加必须拒绝");
    assert!(
        error.message().contains("重复规则数增加"),
        "{}",
        error.message()
    );
}

#[test]
fn evidence_gate_allows_new_distinct_rule() {
    let id = id(ComponentKind::ToolGuidance, "default");
    let mut registry = FakeRegistry::with_entries(vec![(
        id.clone(),
        body_content("# Tool Guidance\n\n- alpha rule\n"),
    )]);
    let change_set = ChangeSet {
        edits: vec![ComponentEdit::Upsert {
            id: id.clone(),
            content: body_content("# Tool Guidance\n\n- alpha rule\n- beta rule\n"),
        }],
    };
    apply_change_set_with_policy(&mut registry, &change_set, WritePolicy::ProseEvidence)
        .expect("新增不重复规则应允许");
}

#[test]
fn maintenance_gate_rejects_token_increase() {
    let id = id(ComponentKind::Guardrails, "default");
    let mut registry = FakeRegistry::with_entries(vec![(
        id.clone(),
        body_content("# Guardrails\n\n- alpha rule\n"),
    )]);
    let change_set = ChangeSet {
        edits: vec![ComponentEdit::Upsert {
            id: id.clone(),
            content: body_content(
                "# Guardrails\n\n- alpha rule\n- a much longer rule that increases tokens\n",
            ),
        }],
    };
    let error =
        apply_change_set_with_policy(&mut registry, &change_set, WritePolicy::ProseMaintenance)
            .expect_err("维护 token 增加必须拒绝");
    assert!(error.message().contains("维护"), "{}", error.message());
}

#[test]
fn evidence_gate_rejects_body_over_budget() {
    let id = id(ComponentKind::Guardrails, "default");
    let mut registry = FakeRegistry::with_entries(vec![(
        id.clone(),
        body_content("# Guardrails\n\n- alpha rule\n"),
    )]);
    let change_set = ChangeSet {
        edits: vec![ComponentEdit::Upsert {
            id: id.clone(),
            content: body_content(&format!(
                "# Guardrails\n\n{}\n",
                (0..120)
                    .map(|index| format!("- {} rule {index}", "长".repeat(60)))
                    .collect::<Vec<_>>()
                    .join("\n")
            )),
        }],
    };
    let error =
        apply_change_set_with_policy(&mut registry, &change_set, WritePolicy::ProseEvidence)
            .expect_err("超预算必须拒绝");
    assert!(error.message().contains("超预算"), "{}", error.message());
}
