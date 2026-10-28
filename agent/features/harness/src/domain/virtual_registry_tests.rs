use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentContent, ComponentKind, ComponentName};
use crate::domain::test_support::FakeRegistry;
use crate::domain::virtual_registry::VirtualComponentRegistry;

fn component_id(name: &str) -> crate::domain::component::ComponentId {
    crate::domain::component::ComponentId {
        kind: ComponentKind::Skill,
        name: ComponentName::new(name),
    }
}

fn body_content(body: &str) -> ComponentContent {
    ComponentContent {
        frontmatter: None,
        body: body.to_string(),
    }
}

/// dry-run 虚拟 registry：虚拟读写/删除只进 overlay，底层真实 registry
/// 内容与写删记录均保持不变。
#[test]
fn virtual_registry_writes_never_reach_base_registry() {
    let alpha = component_id("alpha");
    let beta = component_id("beta");
    let mut base = FakeRegistry::with_entries(vec![(alpha.clone(), body_content("old"))]);

    {
        let mut virtual_registry = VirtualComponentRegistry::new(&mut base);
        virtual_registry
            .write(&alpha, &body_content("new"))
            .expect("虚拟写入");
        virtual_registry
            .write(&beta, &body_content("created"))
            .expect("虚拟新增");
        assert_eq!(virtual_registry.read(&alpha).expect("虚拟读取").body, "new");
        assert_eq!(
            virtual_registry.read(&beta).expect("虚拟新增读取").body,
            "created"
        );
        assert_eq!(virtual_registry.list(), vec![alpha.clone(), beta.clone()]);

        virtual_registry.delete(&alpha).expect("虚拟删除");
        assert!(virtual_registry.read(&alpha).is_err());
        assert_eq!(virtual_registry.list(), vec![beta.clone()]);
    }

    assert_eq!(
        base.content_of(&alpha).map(|content| content.body.clone()),
        Some("old".to_string()),
        "底层组件内容 MUST 不变"
    );
    assert!(base.content_of(&beta).is_none(), "虚拟新增 MUST 不落底座");
    assert!(base.writes.is_empty(), "底层 write MUST 未被调用");
    assert!(base.deletes.is_empty(), "底层 delete MUST 未被调用");
}
