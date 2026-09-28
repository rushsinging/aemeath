//! share::ids 下沉后的 sdk re-export 兼容契约。
//!
//! 契约：`sdk::ids::*` 与顶层 `sdk::*` 必须与 `share::ids` 中的定义是同一类型，
//! 消费方经任一路径引用都在使用同一份 Published Language；re-export 断裂时
//! 本测试在编译期失败。

use sdk::ids;
use sdk::{
    AgentId, ChatId, ChatRunId, IdParseError, InputId, InteractionRequestId, ModelInvocationId,
    RunId, RunStepId, SessionId, ToolCallId,
};

fn assert_same_type<T>(_: T, _: T) {}

#[test]
fn sdk_ids_module_reexports_share_definitions() {
    assert_same_type(
        share::ids::ChatId::new("0198c0de-0000-7000-8000-000000000001"),
        ChatId::new("0198c0de-0000-7000-8000-000000000001"),
    );
    assert_same_type(
        share::ids::ChatRunId::new("0198c0de-0000-7000-8000-000000000002"),
        ChatRunId::new("0198c0de-0000-7000-8000-000000000002"),
    );
    assert_same_type(
        share::ids::RunId::new("0198c0de-0000-7000-8000-000000000003"),
        RunId::new("0198c0de-0000-7000-8000-000000000003"),
    );
    assert_same_type(
        share::ids::SessionId::new("0198c0de-0000-7000-8000-000000000004"),
        SessionId::new("0198c0de-0000-7000-8000-000000000004"),
    );
    assert_same_type(
        share::ids::RunStepId::new("0198c0de-0000-7000-8000-000000000005"),
        RunStepId::new("0198c0de-0000-7000-8000-000000000005"),
    );
    assert_same_type(
        share::ids::ModelInvocationId::new("0198c0de-0000-7000-8000-000000000006"),
        ModelInvocationId::new("0198c0de-0000-7000-8000-000000000006"),
    );
    assert_same_type(
        share::ids::AgentId::new("0198c0de-0000-7000-8000-000000000007"),
        AgentId::new("0198c0de-0000-7000-8000-000000000007"),
    );
    assert_same_type(
        share::ids::InteractionRequestId::new("0198c0de-0000-7000-8000-000000000008"),
        InteractionRequestId::new("0198c0de-0000-7000-8000-000000000008"),
    );
    assert_same_type(
        share::ids::ToolCallId::new("0198c0de-0000-7000-8000-000000000009"),
        ToolCallId::new("0198c0de-0000-7000-8000-000000000009"),
    );
    assert_same_type(
        share::ids::InputId::new("0198c0de-0000-7000-8000-00000000000a"),
        InputId::new("0198c0de-0000-7000-8000-00000000000a"),
    );
}

#[test]
fn sdk_root_reexports_share_id_definitions() {
    assert_same_type(
        share::ids::RunId::new("0198c0de-0000-7000-8000-000000000003"),
        RunId::new("0198c0de-0000-7000-8000-000000000003"),
    );
    assert_same_type(
        share::ids::SessionId::new("0198c0de-0000-7000-8000-000000000004"),
        SessionId::new("0198c0de-0000-7000-8000-000000000004"),
    );
}

#[test]
fn sdk_id_parse_error_reexports_share_definition() {
    assert_same_type(
        share::ids::IdParseError::InvalidUuid("not-a-uuid".to_string()),
        IdParseError::InvalidUuid("not-a-uuid".to_string()),
    );
    assert!(matches!(
        ids::IdParseError::NotVersion7("not-v7".to_string()),
        share::ids::IdParseError::NotVersion7(_)
    ));
}
