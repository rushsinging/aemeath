use super::App;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::workspace::WorktreeKind;

impl App {
    /// Reset per-conversation runtime state while preserving model/provider/session environment.
    pub(crate) async fn reset_runtime_state(&mut self) {
        self.chat.reset_runtime_state();
        self.input.clear_queue();
        // 单一真相源：清空 ConversationModel，使输出文档随之回到空状态。
        self.model.conversation.reset();
        self.refresh_output_widget_from_model();
        self.output_area.reset_runtime_state();
        // 滚动真相归 view_state：清空时同步复位，避免下一帧 adapter 用旧滚动态覆盖 widget。
        self.view_state.output.scroll_to_bottom();
        // 选区真相同样归 view_state：清空时一并清三区选区，避免下一帧 adapter 用旧选区复活 widget 镜像。
        self.view_state.output.clear_selection();
        self.view_state.status_sel.clear_selection();
        self.view_state.input_sel.clear_selection();
        self.status_bar.reset_runtime_state();
        self.input.ask_user_reply_tx = None;
        self.input.ask_user_state = None;
        if let Some(agent_client) = &self.agent_client {
            if let Err(e) = agent_client.sync_current_messages(Vec::new()).await {
                log::warn!("failed to reset SDK session messages: {e}");
            }
            if let Err(e) = agent_client.clear_tasks().await {
                log::warn!("failed to clear SDK task store: {e}");
            }
        }
        self.model
            .runtime
            .apply(crate::tui::model::runtime::intent::RuntimeIntent::UpdateTaskLines(Vec::new()));
    }
    /// Set loaded skills for slash command alias lookup
    pub fn set_skills(&mut self, skills: std::collections::HashMap<String, sdk::SkillView>) {
        self.skills = skills;
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
    /// （spinner + task 同源），此处只更新 Model，不直写 widget。
    pub(crate) async fn update_task_status(&mut self, _is_processing: bool) {
        let lines = match &self.agent_client {
            None => Vec::new(),
            Some(agent_client) => match agent_client.task_status().await {
                Ok(view) => view.lines,
                Err(e) => {
                    log::warn!("failed to fetch SDK task status: {e}");
                    return;
                }
            },
        };
        self.model
            .runtime
            .apply(RuntimeIntent::UpdateTaskLines(lines));
    }

    pub(crate) async fn update_project_context(&mut self) {
        let project = match &self.agent_client {
            None => return,
            Some(agent_client) => agent_client.project(),
        };
        let path_base = empty_to_none(project.path_base);
        let working_root = empty_to_none(project.working_root);
        let kind = match working_root.as_deref() {
            Some(root) if self.session.cwd.as_path() != std::path::Path::new(root) => {
                WorktreeKind::LinkedWorktree
            }
            Some(_) => WorktreeKind::MainCheckout,
            None => WorktreeKind::Unknown,
        };
        self.model.runtime.apply(RuntimeIntent::UpdateWorkspace {
            cwd: project.cwd,
            worktree: None,
        });
        self.model
            .runtime
            .apply(RuntimeIntent::WorkspaceSnapshotReceived {
                path_base,
                working_root,
                branch: project.git_branch,
                kind,
            });
    }

    /// Refresh the cached session list for /resume autocomplete
    pub async fn refresh_session_cache(&mut self) {
        if let Some(agent_client) = &self.agent_client {
            if let Ok(sessions) = agent_client.list_sessions().await {
                self.session.cache_sessions(
                    sessions
                        .iter()
                        .take(20)
                        .map(|s| {
                            let summary = format!("{} [{}msg]", s.summary, s.message_count);
                            (s.id.clone(), summary)
                        })
                        .collect(),
                );
            }
        }
    }

    /// Refresh the cached model list for /model dialog and completion suggestions.
    ///
    /// 模型列表为配置派生、会话期内基本不变的静态数据，启动期预取后由 UI 同步读取，
    /// 消除 dialog/suggestions 在纯路径内的 block_on。
    pub async fn refresh_model_cache(&mut self) {
        if let Some(agent_client) = &self.agent_client {
            if let Ok(models) = agent_client.list_models().await {
                self.session.cache_models(models);
            }
        }
    }
}

fn empty_to_none(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
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

        // 三区真相被清空：避免下一帧 adapter 复活 widget 镜像。
        assert_eq!(app.view_state.output.selection_range(), None);
        assert!(!app.view_state.output.is_selecting());
        assert_eq!(app.view_state.status_sel.selection_range(), None);
        assert!(!app.view_state.status_sel.is_selecting());
        assert_eq!(app.view_state.input_sel.normalized_selection(), None);
        assert!(!app.view_state.input_sel.is_selecting());

        // 经渲染前刷新后，widget 镜像也被同步清空。
        app.refresh_output_scroll_from_view_state();
        assert!(!app.output_area.is_selecting);
        assert!(app.output_area.selection_start.is_none());
        assert!(app.output_area.selection_end.is_none());
    }
}
