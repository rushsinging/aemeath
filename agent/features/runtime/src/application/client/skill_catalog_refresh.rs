//! Skill catalog 轮次边界重扫（Issue #1717）。
//!
//! Skill 正文层由 `FilesystemSkillAdapter` 无缓存实时读取；冻结的是元数据层——
//! TUI slash 补全与路由依赖启动 bootstrap 快照，而 `SkillsUpdated` 事件此前
//! 没有任何发送点。本组件在每轮新 Run 启动前重扫 catalog：
//!
//! - `project_root` 动态取 workspace 当前值（worktree 切换后随之更新）；
//! - `extra_dirs` / `available_tools` 使用启动快照（会话中恒定）；
//! - 仅当 `SkillCatalogSnapshot::revision` 变化时 emit 一次
//!   `RuntimeStreamEvent::SkillsUpdated`，未变化不产生事件。

use crate::application::loop_engine::chat::{ChatEventSink, RuntimeStreamEvent};
use std::sync::{Arc, Mutex};

/// 轮次边界的 skill catalog 刷新器。
#[derive(Clone)]
pub struct SkillCatalogRefresh {
    catalog: Arc<dyn tools::SkillCatalogPort>,
    workspace: project::WorkspaceViews,
    /// 启动快照：extra_dirs 与 available_tools；project_root 字段被忽略，
    /// 每次刷新以 workspace 当前根覆盖。
    query_template: tools::SkillQuery,
    last_revision: Arc<Mutex<String>>,
}

impl SkillCatalogRefresh {
    /// `initial` 为启动 bootstrap 快照，其 revision 作为比较基线。
    pub fn new(
        catalog: Arc<dyn tools::SkillCatalogPort>,
        workspace: project::WorkspaceViews,
        query_template: tools::SkillQuery,
        initial: &tools::SkillCatalogSnapshot,
    ) -> Self {
        Self {
            catalog,
            workspace,
            query_template,
            last_revision: Arc::new(Mutex::new(initial.revision.clone())),
        }
    }

    /// 重扫 catalog；revision 变化时向 `sink` emit `SkillsUpdated` 并返回
    /// 新 snapshot，未变化返回 `None`。
    pub async fn refresh<S: ChatEventSink>(&self, sink: &S) -> Option<tools::SkillCatalogSnapshot> {
        let query = tools::SkillQuery {
            project_root: self.workspace.read().current_workspace_root(),
            ..self.query_template.clone()
        };
        let snapshot = tools::SkillCatalogSnapshot::from_descriptors(self.catalog.list(query));

        let revision_changed = {
            let mut last_revision = self
                .last_revision
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if snapshot.revision == *last_revision {
                return None;
            }
            *last_revision = snapshot.revision.clone();
            true
        };
        debug_assert!(revision_changed);
        log::info!(
            target: crate::LOG_TARGET,
            "skill catalog refreshed: revision changed to {} ({} skills)",
            snapshot.revision,
            snapshot.skills.len()
        );
        sink.send_event(RuntimeStreamEvent::SkillsUpdated {
            snapshot: snapshot.clone(),
        })
        .await;
        Some(snapshot)
    }
}

#[cfg(test)]
#[path = "skill_catalog_refresh_tests.rs"]
mod tests;
