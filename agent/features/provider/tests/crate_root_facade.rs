use provider::{
    InvocationEvent, InvocationRequest, ModelCapability, ModelId, ProviderError, ReasoningLevel,
    RequestSystemBlock,
};

#[test]
fn crate_root_exposes_only_provider_published_language() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<InvocationEvent>();
    assert_send_sync::<InvocationRequest>();
    assert_send_sync::<ModelCapability>();
    assert_send_sync::<ModelId>();
    assert_send_sync::<ProviderError>();
    assert_send_sync::<ReasoningLevel>();
    assert_send_sync::<RequestSystemBlock>();
}
