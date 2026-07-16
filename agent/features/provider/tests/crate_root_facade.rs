use provider::{
    CallbackHandler, LlmClient, LlmError, LlmProvider, LlmProviderGateway, ProviderDriverKind,
    ReasoningLevel, StopReason, StreamHandler, StreamResponse, SystemBlock, Usage,
};

#[test]
fn crate_root_exposes_existing_provider_contract() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LlmClient>();
    assert_send_sync::<LlmError>();
    assert_send_sync::<ProviderDriverKind>();
    assert_send_sync::<ReasoningLevel>();
    assert_send_sync::<StopReason>();
    assert_send_sync::<StreamResponse>();
    assert_send_sync::<SystemBlock>();
    assert_send_sync::<Usage>();

    let _: Option<&dyn LlmProvider> = None;
    let _: Option<&dyn LlmProviderGateway> = None;
    let _: Option<&dyn StreamHandler> = None;
    let _ = CallbackHandler::new(Box::new(|_| {}));
}
