//! SDK chat ingress/egress adapter assembly.

/// Assemble the concrete SDK input and event adapters at the Runtime adapter boundary.
pub fn wire_sdk_chat_ingress() -> crate::application::client::RuntimeIngressAssembly {
    crate::application::client::RuntimeIngressAssembly::new(
        std::sync::Arc::new(|sender| {
            crate::application::loop_engine::chat::ChatEventSinkHandle::new(
                crate::adapters::sdk_event_sink::SdkChatEventSink::new(sender),
            )
        }),
        std::sync::Arc::new(|ingress| {
            crate::application::client::SessionInputHandle::new(
                crate::adapters::input_buffer::RuntimeInputEventDrainPort::new(ingress),
            )
        }),
    )
}
