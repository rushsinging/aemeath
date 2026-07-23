use super::App;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::runtime::status_notice::StatusNotice;
use crate::tui::model::workspace_provider::WorkspaceIntent;
use crate::tui::update::intent::AgentIntent;

impl App {
    /// Reset per-conversation runtime state while preserving model/provider/session environment.
    pub(crate) async fn reset_runtime_state(&mut self) {
        self.chat.reset_runtime_state();
        // 单一真相源：清空 ConversationModel，使输出文档随之回到空状态。
        self.model.conversation.reset();
        // 清 assemble memo cache：reset 后 revision 归 0，若旧 cache 恰好 revision==0 会误命中。
        // 显式清除消除隐式依赖，语义更明确（#425 review Fix 2）。
        self.output_view_cache = None;
        self.mark_output_dirty();
        self.output_area.reset_runtime_state();
        // 滚动真相归 view_state：清空时同步复位，避免下一帧 adapter 用旧滚动态覆盖 widget。
        self.view_state.output.scroll_to_bottom();
        // 选区真相同样归 view_state：清空时一并清三区选区，避免下一帧 adapter 用旧选区复活 widget 镜像。
        self.view_state.output.clear_selection();
        self.view_state.status_sel.clear_selection();
        self.view_state.input_sel.clear_selection();
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::SetStatusNotice(SetStatusNotice(StatusNotice::ready())),
        ));
        // Reset 事件由 runtime gate 处理（清 chain + clear_tasks）。
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::UpdateTaskLines(UpdateTaskLines(Vec::new())),
        ));
    }
    /// Set loaded skills for slash command alias lookup
    pub fn set_skills(&mut self, skills: std::collections::HashMap<String, sdk::SkillView>) {
        self.skills = skills;
    }

    pub fn set_commands(
        &mut self,
        catalog: std::sync::Arc<dyn sdk::CommandCatalogPort>,
        router: std::sync::Arc<dyn sdk::CommandRouterPort>,
    ) {
        self.command_catalog = Some(catalog);
        self.command_router = Some(router);
    }

    /// Find a skill by its name or alias
    pub(crate) fn find_skill_by_alias(&self, alias: &str) -> Option<&sdk::SkillView> {
        self.skills
            .values()
            .find(|s| s.name == alias || s.aliases.iter().any(|a| a == alias))
    }

    /// Update task status display in output area. Also runs lifecycle checks.
    ///
    /// 单一真相：task 行真相归 `RuntimeModel.task_status.lines`，经 `UpdateTaskLines`
    /// intent 写入。widget 镜像由每帧 `refresh_live_status_from_model` 统一写回
    pub(crate) async fn update_project_context(&mut self) {
        // #567：project() 已从 trait 删除，workspace_root 从 TuiLaunchContext 获取。
        // 项目上下文通过 ProjectInfo 事件推送（后续 PR 实现）。
        // 暂时从 session.cwd 获取。
        let workspace_root = self.session.cwd.as_path().to_path_buf();
        self.apply_agent_intent(AgentIntent::Workspace(WorkspaceIntent::SetCurrent {
            cwd: self.session.cwd.to_string_lossy().to_string(),
            worktree: None,
        }));
        self.apply_agent_intent(AgentIntent::Workspace(WorkspaceIntent::ApplySnapshot {
            path_base: None,
            workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::App;
    use sdk::CharIdx;
    use std::path::PathBuf;

    fn test_app() -> App {
        App::new(
            "test-session".to_string(),
            PathBuf::from("/tmp"),
            "test-model".to_string(),
        )
    }

    #[tokio::test]
    async fn reset_runtime_state_clears_view_state_selection_truth() {
        use crate::tui::render::status::StatusBarRow;
        let mut app = test_app();
        // 在 view_state（三区选区真相）中建立选区。
        app.view_state.output.begin_selection(0, CharIdx::new(1));
        app.view_state.output.update_selection(0, CharIdx::new(5));
        assert!(app.view_state.output.selection_range().is_some());
        app.view_state
            .status_sel
            .begin_selection(StatusBarRow::Runtime, 2, 80);
        app.view_state.status_sel.update_selection(6);
        assert!(app.view_state.status_sel.selection_range().is_some());
        app.view_state.input_sel.begin_selection((0, 2));
        app.view_state.input_sel.update_selection((0, 6));
        assert!(app.view_state.input_sel.normalized_selection().is_some());

        app.reset_runtime_state().await;

        // 三区真相被清空。
        assert_eq!(app.view_state.output.selection_range(), None);
        assert!(!app.view_state.output.is_selecting());
        assert_eq!(app.view_state.status_sel.selection_range(), None);
        assert!(!app.view_state.status_sel.is_selecting());
        assert_eq!(app.view_state.input_sel.normalized_selection(), None);
        assert!(!app.view_state.input_sel.is_selecting());
    }
}
