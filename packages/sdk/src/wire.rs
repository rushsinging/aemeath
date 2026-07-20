//! 可传输 SDK Published Language 的 JSON Schema components。
//!
//! 本模块只导出纯值 command、event、snapshot、outcome 与 identity schema；
//! 不定义 HTTP/WS 操作、端点或 Server 传输协议。

use schemars::{schema_for, JsonSchema};
use serde_json::{json, Map, Value};

use crate::{
    CancelRunOutcome, CancelRunStepOutcome, ChatEventContext, ChatMessage, ConfigChangedEvent,
    ConfigUpdate, ConfigUpdateResult, ConfigView, ControlDeadline, HookMessageView,
    InteractionCancelReason, InteractionCommandOutcome, InteractionReply, InteractionRequest,
    InteractionRequestBody, ModelSummary, ProjectContext, ReflectionHistoryView,
    RunTerminationReason, SessionResumeFailureKind, SessionSnapshot, SessionSummary,
    TerminateRunOutcome, WorkspaceContextView,
};

/// 生成供未来 Server adapter 组装 OpenAPI components 的 JSON Schema 文档。
///
/// 这里不冻结 `paths`、`servers` 或任意传输语义；这些属于 Server Future 设计。
pub fn components_document() -> Value {
    let mut definitions = Map::new();
    register::<InteractionRequest>(&mut definitions);
    register::<InteractionRequestBody>(&mut definitions);
    register::<InteractionReply>(&mut definitions);
    register::<InteractionCommandOutcome>(&mut definitions);
    register::<InteractionCancelReason>(&mut definitions);
    register::<CancelRunOutcome>(&mut definitions);
    register::<CancelRunStepOutcome>(&mut definitions);
    register::<TerminateRunOutcome>(&mut definitions);
    register::<RunTerminationReason>(&mut definitions);
    register::<ControlDeadline>(&mut definitions);
    register::<ConfigView>(&mut definitions);
    register::<ConfigUpdate>(&mut definitions);
    register::<ConfigUpdateResult>(&mut definitions);
    register::<ConfigChangedEvent>(&mut definitions);
    register::<ProjectContext>(&mut definitions);
    register::<ModelSummary>(&mut definitions);
    register::<ReflectionHistoryView>(&mut definitions);
    register::<ChatEventContext>(&mut definitions);
    register::<SessionSummary>(&mut definitions);
    register::<SessionSnapshot>(&mut definitions);
    register::<ChatMessage>(&mut definitions);
    register::<WorkspaceContextView>(&mut definitions);
    register::<HookMessageView>(&mut definitions);
    register::<SessionResumeFailureKind>(&mut definitions);

    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Aemeath Agent Runtime Wire Components",
        "type": "object",
        "$defs": definitions,
    })
}

fn register<T: JsonSchema>(definitions: &mut Map<String, Value>) {
    let schema = serde_json::to_value(schema_for!(T)).expect("JsonSchema 必须可序列化为 JSON");
    let root_name = T::schema_name().into_owned();

    if let Some(definitions_value) = schema.get("$defs").and_then(Value::as_object) {
        for (name, definition) in definitions_value {
            definitions.insert(name.clone(), definition.clone());
        }
    }
    definitions.insert(root_name, without_root_definitions(schema));
}

fn without_root_definitions(mut schema: Value) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object.remove("$defs");
        object.remove("$schema");
    }
    schema
}
