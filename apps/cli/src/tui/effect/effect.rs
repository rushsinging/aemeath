#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    None,
    RequestRender,
    SpawnAgentChat { chat_id: String, prompt: String },
    SaveSession,
    FetchTaskStatus,
    CopyToClipboard { text: String },
    RunHook { name: String },
    StartTimer { id: String },
    StopTimer { id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_request_render_is_pure_value() {
        let effect = Effect::RequestRender;
        assert_eq!(format!("{effect:?}"), "RequestRender");
    }

    #[test]
    fn test_spawn_agent_chat_carries_chat_id() {
        let effect = Effect::SpawnAgentChat {
            chat_id: "chat-1".to_string(),
            prompt: "hello".to_string(),
        };
        assert!(matches!(effect, Effect::SpawnAgentChat { ref chat_id, .. } if chat_id == "chat-1"));
    }

    #[test]
    fn test_effect_none_is_distinct_from_render() {
        assert_ne!(Effect::None, Effect::RequestRender);
    }
}
