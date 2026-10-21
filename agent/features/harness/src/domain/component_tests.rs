//! ComponentId / ComponentContent / diff 契约测试。

use crate::domain::component::{
    diff_contents, ComponentContent, ComponentId, ComponentKind, ComponentName, ContentChange,
};

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

/// list() 的稳定排序契约：先按 kind（枚举声明序），再按 name 升序。
#[test]
fn component_ids_sort_by_kind_then_name() {
    let mut ids = [
        id(ComponentKind::Skill, "zeta"),
        id(ComponentKind::Guardrails, "bbb"),
        id(ComponentKind::Skill, "alpha"),
        id(ComponentKind::Guardrails, "aaa"),
    ];
    ids.sort();
    let rendered: Vec<String> = ids.iter().map(|component| component.to_string()).collect();
    assert_eq!(
        rendered,
        vec![
            "guardrails/aaa",
            "guardrails/bbb",
            "skill/alpha",
            "skill/zeta",
        ],
    );
}

/// diff 契约：Added / Removed / Unchanged / Modified 四态判定。
#[test]
fn diff_contents_classifies_added_removed_unchanged_modified() {
    let before = body_content("same");
    let after_same = body_content("same");
    let after_other = body_content("changed");

    assert_eq!(diff_contents(None, Some(&after_same)), ContentChange::Added);
    assert_eq!(diff_contents(Some(&before), None), ContentChange::Removed);
    assert_eq!(
        diff_contents(Some(&before), Some(&after_same)),
        ContentChange::Unchanged,
    );
    assert_eq!(
        diff_contents(Some(&before), Some(&after_other)),
        ContentChange::Modified,
    );
}

/// frontmatter 变化也构成 Modified（frontmatter 是结构化正文的一部分）。
#[test]
fn diff_contents_treats_frontmatter_change_as_modified() {
    let before = ComponentContent {
        frontmatter: None,
        body: "same body".to_string(),
    };
    let after = ComponentContent {
        frontmatter: Some(serde_json::json!({"name": "x"})),
        body: "same body".to_string(),
    };
    assert_eq!(
        diff_contents(Some(&before), Some(&after)),
        ContentChange::Modified,
    );
}
