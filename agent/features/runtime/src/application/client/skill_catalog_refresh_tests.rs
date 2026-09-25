//! `SkillCatalogRefresh` 的行为测试：轮次边界重扫 skill 目录，
/// 仅在 catalog revision 变化时 emit 一次 `SkillsUpdated`。
use std::sync::{Arc, Mutex};

use crate::application::client::SkillCatalogRefresh;
use crate::application::loop_engine::chat::{ChatEventSink, EventFuture, RuntimeStreamEvent};
use tools::{SkillCatalogPort, SkillDescriptor, SkillQuery, SkillSource, SkillSourceKind};

/// 可变 catalog fake：模拟磁盘上 skill 集合在轮次间变化。
struct MutableCatalog {
    descriptors: Mutex<Vec<SkillDescriptor>>,
}

impl SkillCatalogPort for MutableCatalog {
    fn list(&self, _query: SkillQuery) -> Vec<SkillDescriptor> {
        self.descriptors
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
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

fn workspace_views() -> project::WorkspaceViews {
    project::wire_production_workspace(std::env::temp_dir(), None)
        .expect("wire test workspace")
        .into_views()
}

fn refresh_with(
    descriptors: Vec<SkillDescriptor>,
) -> (Arc<MutableCatalog>, SkillCatalogRefresh, CollectingSink) {
    let catalog = Arc::new(MutableCatalog {
        descriptors: Mutex::new(descriptors.clone()),
    });
    let initial = tools::SkillCatalogSnapshot::from_descriptors(descriptors);
    let query = SkillQuery::new(std::env::temp_dir(), Vec::new(), Default::default());
    let refresh = SkillCatalogRefresh::new(catalog.clone(), workspace_views(), query, &initial);
    (catalog, refresh, CollectingSink::default())
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
        .descriptors
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(descriptor("release", "new skill"));
    let first = refresh.refresh(&sink).await;
    assert!(first.is_some(), "revision 变化后必须返回新 snapshot");
    assert_eq!(first.expect("checked some").revision, {
        tools::SkillCatalogSnapshot::from_descriptors(vec![
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
        .descriptors
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
