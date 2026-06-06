use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Debug, Default)]
struct ToolIdentityState {
    by_stream_index: HashMap<usize, String>,
    by_provider_id: HashMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct ToolIdentityRegistry {
    next_id: Arc<AtomicUsize>,
    state: Arc<Mutex<ToolIdentityState>>,
}

impl ToolIdentityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn runtime_id_for_stream(&self, index: usize, provider_id: Option<&str>) -> String {
        let mut state = self.state.lock().expect("tool identity registry poisoned");
        if let Some(provider_id) = provider_id.filter(|id| !id.is_empty()) {
            if let Some(id) = state.by_provider_id.get(provider_id).cloned() {
                state.by_stream_index.insert(index, id.clone());
                return id;
            }
            let id = self.next_runtime_id();
            state.by_stream_index.insert(index, id.clone());
            state
                .by_provider_id
                .insert(provider_id.to_string(), id.clone());
            return id;
        }
        if let Some(id) = state.by_stream_index.get(&index).cloned() {
            return id;
        }
        let id = self.next_runtime_id();
        state.by_stream_index.insert(index, id.clone());
        id
    }

    pub fn runtime_id_for_provider(&self, provider_id: &str) -> String {
        let mut state = self.state.lock().expect("tool identity registry poisoned");
        if let Some(id) = state.by_provider_id.get(provider_id).cloned() {
            return id;
        }
        let id = self.next_runtime_id();
        state
            .by_provider_id
            .insert(provider_id.to_string(), id.clone());
        id
    }

    fn next_runtime_id(&self) -> String {
        let next = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("tool-{next}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_id_for_stream_reuses_provider_mapping() {
        let registry = ToolIdentityRegistry::new();
        let first = registry.runtime_id_for_stream(0, Some("provider-a"));
        let second = registry.runtime_id_for_stream(3, Some("provider-a"));

        assert_eq!(first, second);
    }

    #[test]
    fn test_runtime_id_for_provider_reuses_stream_mapping() {
        let registry = ToolIdentityRegistry::new();
        let streamed = registry.runtime_id_for_stream(0, Some("provider-a"));
        let final_id = registry.runtime_id_for_provider("provider-a");

        assert_eq!(streamed, final_id);
    }

    #[test]
    fn test_runtime_id_for_provider_allocates_unique_ids() {
        let registry = ToolIdentityRegistry::new();
        let first = registry.runtime_id_for_provider("provider-a");
        let second = registry.runtime_id_for_provider("provider-b");

        assert_ne!(first, second);
    }
}
