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
        fn assert_send_sync<T: Send + Sync + ?Sized>(_: &T) {}

        let gateway: Arc<dyn LlmProviderGateway> = wire_provider();

        assert_send_sync(gateway.as_ref());
    }
}
