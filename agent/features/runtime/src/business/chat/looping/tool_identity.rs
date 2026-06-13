use sdk::ids::ToolCallId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct ToolIdentityState {
    by_stream_index: HashMap<usize, ToolCallId>,
    by_provider_id: HashMap<String, ToolCallId>,
}

#[derive(Clone, Debug, Default)]
pub struct ToolIdentityRegistry {
    state: Arc<Mutex<ToolIdentityState>>,
}

impl ToolIdentityRegistry {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ToolIdentityState::default())),
        }
    }

    pub fn runtime_id_for_stream(&self, index: usize, provider_id: Option<&str>) -> ToolCallId {
        let mut state = self.state.lock().expect("tool identity registry poisoned");

        if let Some(provider_id) = provider_id.filter(|id| !id.is_empty()) {
            if let Some(id) = state.by_provider_id.get(provider_id).cloned() {
                state.by_stream_index.insert(index, id.clone());
                return id;
            }
            let id = ToolCallId::new_v7();
            state.by_stream_index.insert(index, id.clone());
            state.by_provider_id.insert(provider_id.to_string(), id.clone());
            return id;
        }

        if let Some(id) = state.by_stream_index.get(&index).cloned() {
            return id;
        }

        let id = ToolCallId::new_v7();
        state.by_stream_index.insert(index, id.clone());
        id
    }

    pub fn runtime_id_for_provider(&self, provider_id: &str) -> ToolCallId {
        let mut state = self.state.lock().expect("tool identity registry poisoned");

        if let Some(id) = state.by_provider_id.get(provider_id).cloned() {
            return id;
        }

        let id = ToolCallId::new_v7();
        state.by_provider_id.insert(provider_id.to_string(), id.clone());
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_same_provider_id_reuses_same_tool_call_id() {
        let registry = ToolIdentityRegistry::new();
        let id1 = registry.runtime_id_for_provider("provider-a");
        let id2 = registry.runtime_id_for_provider("provider-a");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_different_provider_ids_generate_different_tool_call_ids() {
        let registry = ToolIdentityRegistry::new();
        let id1 = registry.runtime_id_for_provider("provider-a");
        let id2 = registry.runtime_id_for_provider("provider-b");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_stream_index_binds_to_provider_id_later() {
        let registry = ToolIdentityRegistry::new();
        let id_by_index = registry.runtime_id_for_stream(0, None);
        let id_by_provider = registry.runtime_id_for_stream(0, Some("provider-a"));
        assert_eq!(id_by_index, id_by_provider);
    }

    #[test]
    fn test_all_ids_are_uuidv7() {
        let registry = ToolIdentityRegistry::new();
        let id1 = registry.runtime_id_for_stream(0, None);
        let id2 = registry.runtime_id_for_provider("provider-a");
        assert_eq!(id1.as_uuid().get_version_num(), 7);
        assert_eq!(id2.as_uuid().get_version_num(), 7);
    }
}
