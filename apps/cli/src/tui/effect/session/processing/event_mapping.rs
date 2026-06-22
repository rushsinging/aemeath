use crate::tui::app::event::{StatusContextUpdate, UiEvent};

pub(crate) fn sdk_event_to_ui_event(event: sdk::ChatEvent) -> UiEvent {
    match event {
        sdk::ChatEvent::Token { context, text } => UiEvent::Text {
            context: context.into(),
            text,
        },
        sdk::ChatEvent::Thinking { context, text } => UiEvent::Thinking {
            context: context.into(),
            text,
        },
        sdk::ChatEvent::BlockComplete { context, text } => UiEvent::BlockComplete {
            context: context.into(),
            text,
        },
        sdk::ChatEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => UiEvent::ToolCallStart {
            context: context.into(),
            id,
            provider_id,
            name,
            index,
        },
        sdk::ChatEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => UiEvent::ToolCallUpdate {
            context: context.into(),
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        },
        sdk::ChatEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
            ..
        } => UiEvent::ToolResult {
            context: context.into(),
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        },
        sdk::ChatEvent::SystemMessage(msg) => UiEvent::SystemMessage(msg),
        sdk::ChatEvent::Error(msg) => UiEvent::Error(msg),
        sdk::ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => UiEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        },
        sdk::ChatEvent::MessagesSync(messages) => UiEvent::MessagesSync(messages),
        sdk::ChatEvent::Done { context } => UiEvent::Done {
            context: context.into(),
        },
        sdk::ChatEvent::DoneWithDurationMs {
            context,
            duration_ms,
        } => UiEvent::DoneWithDuration {
            context: context.into(),
            duration: std::time::Duration::from_millis(duration_ms),
        },
        sdk::ChatEvent::Cancelled { context } => UiEvent::Cancelled {
            context: context.into(),
        },
        sdk::ChatEvent::LiveTps(tps) => UiEvent::LiveTps(tps),
        sdk::ChatEvent::CurrentTurnChanged(turn) | sdk::ChatEvent::TurnChanged(turn) => {
            UiEvent::CurrentTurnChanged(turn)
        }
        sdk::ChatEvent::HookEvent(event) => UiEvent::HookEvent(event),
        sdk::ChatEvent::AskUser {
            id,
            question,
            options,
            allow_free_input: _,
            multi_select,
            default,
            reply_tx,
        } => UiEvent::AskUser {
            id: sdk::ids::ToolCallId::from_legacy_or_new(&id),
            question,
            options,
            multi_select,
            default,
            reply_tx,
        },
        sdk::ChatEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => UiEvent::AgentProgress {
            context: context.into(),
            tool_id,
            event,
        },
        sdk::ChatEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => UiEvent::WorkingDirectoryChanged(StatusContextUpdate {
            path_base: crate::tui::app::display_status_path(std::path::Path::new(&path_base)),
            workspace_root: crate::tui::app::display_status_path(std::path::Path::new(&workspace_root)),
            branch: crate::tui::app::git_branch_for(std::path::Path::new(&workspace_root)),
            kind: crate::tui::app::worktree_kind_for(std::path::Path::new(&workspace_root)),
            raw_path_base: std::path::PathBuf::from(path_base),
            raw_workspace_root: std::path::PathBuf::from(workspace_root),
            workspace,
        }),
        sdk::ChatEvent::TasksChanged => UiEvent::TaskStatusChanged,
        sdk::ChatEvent::ConfigReloaded { changed_keys } => {
            let keys_str = changed_keys.join(", ");
            UiEvent::SystemMessage(format!("[config reloaded] changed: {}", keys_str))
        }
        sdk::ChatEvent::SessionReset => UiEvent::SessionReset,
        sdk::ChatEvent::UserMessagesWithdrawn { texts } => {
            UiEvent::UserMessagesWithdrawn(texts)
        }
        sdk::ChatEvent::Result(result) => UiEvent::SystemMessage(result.text),
    }
}
