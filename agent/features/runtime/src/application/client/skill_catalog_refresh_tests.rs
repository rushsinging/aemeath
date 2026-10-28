//! `SkillCatalogRefresh` 的行为测试：轮次边界重扫 skill 目录，
/// 仅在 catalog revision 变化时 emit 一次 `SkillsUpdated`。
use std::sync::{Arc, Mutex};

use crate::application::client::SkillCatalogRefresh;
use crate::application::loop_engine::chat::{ChatEventSink, EventFuture, RuntimeStreamEvent};
use tools::published::skill::{
    SkillCatalogPort, SkillDescriptor, SkillQuery, SkillSource, SkillSourceKind,
};

/// 可变 catalog 替身（#2146：实现权归 tools crate；跨 crate 经脚本端口）。
fn mutable_catalog(descriptors: Arc<Mutex<Vec<SkillDescriptor>>>) -> Arc<dyn SkillCatalogPort> {
    let as_trait: Arc<dyn SkillCatalogPort> =
        Arc::new(tools::test_support::MutableSkillCatalog(descriptors));
    as_trait
}
/// 收集全部事件的 sink fake。
#[derive(Clone, Default)]
struct CollectingSink {
    events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
}

impl ChatEventSink for CollectingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        self.try_send_event(event);
        Box::pin(std::future::ready(()))
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(event);
    }
}

fn descriptor(name: &str, description: &str) -> SkillDescriptor {
    SkillDescriptor::new(
        name.to_string(),
        description.to_string(),
        SkillSource::file(SkillSourceKind::ProjectAgents, "fixture".to_string()),
        Vec::new(),
        Some(name.to_string()),
        Vec::new(),
        None,
    )
}

fn workspace_views() -> project::Workspace {
    project::wire_production_workspace(std::env::temp_dir(), None).expect("wire test workspace")
}

fn refresh_with(
    descriptors: Vec<SkillDescriptor>,
) -> (
    Arc<std::sync::Mutex<Vec<SkillDescriptor>>>,
    SkillCatalogRefresh,
    CollectingSink,
) {
    let descriptors = Arc::new(std::sync::Mutex::new(descriptors.clone()));
    let catalog = mutable_catalog(Arc::clone(&descriptors));
    let initial = tools::published::skill::SkillCatalogSnapshot::from_descriptors(
        descriptors.lock().unwrap().clone(),
    );
    let query = SkillQuery::new(std::env::temp_dir(), Vec::new(), Default::default());
    let refresh = SkillCatalogRefresh::new(catalog, workspace_views(), query, &initial);
    (descriptors, refresh, CollectingSink::default())
}

fn skills_updated_count(sink: &CollectingSink) -> usize {
    sink.events
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter(|event| matches!(event, RuntimeStreamEvent::SkillsUpdated { .. }))
        .count()
}

#[tokio::test]
async fn unchanged_revision_does_not_emit() {
    let (_catalog, refresh, sink) = refresh_with(vec![descriptor("commit", "desc")]);

    let emitted = refresh.refresh(&sink).await;

    assert!(emitted.is_none());
    assert_eq!(skills_updated_count(&sink), 0);
}

#[tokio::test]
async fn changed_revision_emits_once_until_next_change() {
    let (catalog, refresh, sink) = refresh_with(vec![descriptor("commit", "desc")]);

    catalog
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(descriptor("release", "new skill"));
    let first = refresh.refresh(&sink).await;
    assert!(first.is_some(), "revision 变化后必须返回新 snapshot");
    assert_eq!(first.expect("checked some").revision, {
        tools::published::skill::SkillCatalogSnapshot::from_descriptors(vec![
            descriptor("commit", "desc"),
            descriptor("release", "new skill"),
        ])
        .revision
    });

    // 同 revision 重复刷新：不再 emit。
    let second = refresh.refresh(&sink).await;
    assert!(second.is_none());

    assert_eq!(skills_updated_count(&sink), 1);
}

#[tokio::test]
async fn removed_skill_emits_updated() {
    let (catalog, refresh, sink) = refresh_with(vec![
        descriptor("commit", "desc"),
        descriptor("release", "desc"),
    ]);

    catalog
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .retain(|skill| skill.name() != "release");
    let emitted = refresh.refresh(&sink).await;

    assert!(emitted.is_some());
    assert_eq!(skills_updated_count(&sink), 1);
    let released_gone = {
        let events = sink
            .events
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        events.iter().any(|event| match event {
            RuntimeStreamEvent::SkillsUpdated { snapshot } => !snapshot
                .skills
                .iter()
                .any(|skill| skill.name() == "release"),
            _ => false,
        })
    };
    assert!(
        released_gone,
        "SkillsUpdated snapshot 不应再包含被删除的 skill"
    );
}
