use std::sync::Arc;

use ::provider::api::LlmProviderGateway;

pub fn wire_provider() -> Arc<dyn LlmProviderGateway> {
    ::provider::api::wire_provider()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_provider_returns_object_safe_gateway() {
        let gateway: Arc<dyn LlmProviderGateway> = wire_provider();

        assert_eq!(Arc::strong_count(&gateway), 1);
    }
}
