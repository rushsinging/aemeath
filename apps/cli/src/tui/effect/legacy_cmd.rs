use crate::tui::core::msg::Cmd;

use super::effect::Effect;

pub(crate) fn effect_from_legacy_cmd(cmd: &Cmd) -> Effect {
    match cmd {
        Cmd::None => Effect::None,
        Cmd::Quit => Effect::RequestRender,
        Cmd::SpawnProcessing(_) => Effect::SpawnAgentChat {
            chat_id: "legacy-processing".to_string(),
            prompt: String::new(),
        },
        Cmd::SaveCurrentSession => Effect::SaveSession,
        Cmd::RunHookNotification { kind, .. } => Effect::RunHook { name: kind.clone() },
        Cmd::ReadClipboardImage | Cmd::ProcessImageFile(_) => Effect::CopyToClipboard {
            text: String::new(),
        },
        Cmd::SetCurrentTurn(_) => Effect::RequestRender,
        Cmd::FetchReminderRecap => Effect::FetchTaskStatus,
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::core::msg::Cmd;
    use crate::tui::effect::effect::Effect;

    use super::effect_from_legacy_cmd;

    #[test]
    fn test_none_cmd_maps_to_none_effect() {
        assert_eq!(effect_from_legacy_cmd(&Cmd::None), Effect::None);
    }

    #[test]
    fn test_save_session_maps_to_save_session_effect() {
        assert_eq!(effect_from_legacy_cmd(&Cmd::SaveCurrentSession), Effect::SaveSession);
    }

    #[test]
    fn test_hook_notification_maps_kind_to_hook_name() {
        let effect = effect_from_legacy_cmd(&Cmd::RunHookNotification {
            message: "done".to_string(),
            kind: "Stop".to_string(),
        });
        assert_eq!(
            effect,
            Effect::RunHook {
                name: "Stop".to_string()
            }
        );
    }
}
