use crate::tui::app::UiEvent;
use kernel::config::hooks::{HookEntry, HookEvent};
use kernel::hook::{HookData, HookJsonOutput, HookResult, HookRunner};
use tokio::sync::mpsc;

#[derive(Clone)]
pub(crate) struct HookUi {
    tx: mpsc::Sender<UiEvent>,
}

impl HookUi {
    pub(crate) fn new(tx: mpsc::Sender<UiEvent>) -> Self {
        Self { tx }
    }

    pub(crate) async fn run_json(
        &self,
        runner: &HookRunner,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        let hooks = runner.matching_hooks(event, tool_name);
        if hooks.is_empty() {
            return Vec::new();
        }

        let command = hooks
            .first()
            .map(|hook| hook.command.clone())
            .unwrap_or_default();
        let event_name = hook_event_name(event);
        let _ = self
            .tx
            .send(UiEvent::HookStart {
                event: event_name.to_string(),
                command,
            })
            .await;

        let hook_results = runner.run_hooks_with_json(event, tool_name, data).await;

        for (_, result, _) in &hook_results {
            let _ = self
                .tx
                .send(UiEvent::HookEnd {
                    event: event_name.to_string(),
                    blocked: result.blocked,
                    error: result.error.clone(),
                })
                .await;
        }
        hook_results
    }

    pub(crate) async fn run_plain(
        &self,
        runner: &HookRunner,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
    ) -> Vec<HookResult> {
        self.run_json(runner, event, tool_name, data)
            .await
            .into_iter()
            .map(|(_, result, _)| result)
            .collect()
    }
}

pub(crate) fn hook_event_name(event: HookEvent) -> &'static str {
    match event {
        HookEvent::PreToolUse => "PreToolUse",
        HookEvent::PostToolUse => "PostToolUse",
        HookEvent::PostToolUseFailure => "PostToolUseFailure",
        HookEvent::UserPromptSubmit => "UserPromptSubmit",
        HookEvent::Stop => "Stop",
        HookEvent::StopFailure => "StopFailure",
        HookEvent::SessionStart => "SessionStart",
        HookEvent::SessionEnd => "SessionEnd",
        HookEvent::PreCompact => "PreCompact",
        HookEvent::PostCompact => "PostCompact",
        HookEvent::PostToolBatch => "PostToolBatch",
        HookEvent::SubagentStart => "SubagentStart",
        HookEvent::SubagentStop => "SubagentStop",
        HookEvent::TaskCreated => "TaskCreated",
        HookEvent::TaskCompleted => "TaskCompleted",
        HookEvent::PermissionRequest => "PermissionRequest",
        HookEvent::PermissionDenied => "PermissionDenied",
        HookEvent::Notification => "Notification",
        HookEvent::InstructionsLoaded => "InstructionsLoaded",
        HookEvent::ConfigChange => "ConfigChange",
        HookEvent::Elicitation => "Elicitation",
        HookEvent::ElicitationResult => "ElicitationResult",
        HookEvent::UserPromptExpansion => "UserPromptExpansion",
        HookEvent::CwdChanged => "CwdChanged",
        HookEvent::FileChanged => "FileChanged",
        HookEvent::TeammateIdle => "TeammateIdle",
    }
}
