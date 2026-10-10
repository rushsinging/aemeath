use super::*;
use crate::application::model::test_support::text_completion_stream;
use crate::ports::provider_port::{
    ModelInfo, ProviderError, ProviderRequestData, ProviderResponseStream,
};
use async_trait::async_trait;
use memory::api::reflection::{ReflectionRecord, ReflectionSafeSummary};
use memory::api::{MemoryError, NoOpMemory, ReflectionHistoryQuery};
use std::collections::VecDeque;
use std::sync::Mutex;

struct StaticProvider {
    response: String,
}

#[async_trait]
impl ProviderPort for StaticProvider {
    // `capabilities()` 已删除（#1880）：binding 持全量 ModelInfo，运行时零查询
    // ——unknown model 门禁语义已在装配时。

    async fn invoke(
        &self,
        _request: ProviderRequestData,
        _cancel: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<ProviderResponseStream, ProviderError> {
        Ok(text_completion_stream(self.response.clone(), 11, 22))
    }
}

#[derive(Default)]
struct RecordingHistory {
    records: Mutex<Vec<ReflectionRecord>>,
}

#[async_trait]
impl ReflectionHistoryQuery for RecordingHistory {
    async fn list(&self, limit: usize) -> Result<Vec<ReflectionSafeSummary>, MemoryError> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .rev()
            .take(limit)
            .map(ReflectionRecord::safe_summary)
            .collect())
    }
}

#[async_trait]
impl ReflectionHistoryStore for RecordingHistory {
    async fn append(&self, record: &ReflectionRecord) -> Result<(), MemoryError> {
        self.records.lock().unwrap().push(record.clone());
        Ok(())
    }

    async fn upsert(&self, record: &ReflectionRecord) -> Result<(), MemoryError> {
        let mut records = self.records.lock().unwrap();
        if let Some(existing) = records.iter_mut().find(|item| item.id == record.id) {
            *existing = record.clone();
        } else {
            records.push(record.clone());
        }
        Ok(())
    }
}

fn model() -> ModelInfo {
    ModelInfo {
        provider: "reflection-test-provider".to_string(),
        model: "reflection-test-model".to_string(),
        supports_tools: false,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        supported_reasoning: vec![share::reasoning::ReasoningLevel::Off],
        context_limit: Some(8_192),
        output_limit: Some(4_096),
    }
}

fn identity() -> ReflectionExecutionIdentity {
    ReflectionExecutionIdentity {
        id: "reflection-id".to_string(),
        timestamp: 42,
        trigger: memory::api::reflection::ReflectionTrigger::Manual,
        coverage_end: None,
    }
}

#[tokio::test]
async fn runtime_invokes_provider_then_delegates_parse_apply_and_history_to_memory() {
    let provider = StaticProvider {
        response: r#"{"deviations":["drift"],"suggested_memories":[]}"#.to_string(),
    };
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let model = model();
    let result = execute_reflection(
        &[share::message::Message::user("reflect")],
        "en",
        false,
        ReflectionInvocation {
            provider: &provider,
            model: &model,
            max_tokens: 4_096,
            requested_reasoning: share::reasoning::ReasoningLevel::Off,
            system_prompt_text: "system",
        },
        &NoOpMemory,
        &history,
        &identity(),
        &cancel,
    )
    .await
    .unwrap();

    assert_eq!(result.output.deviations, ["drift"]);
    assert_eq!((result.input_tokens, result.output_tokens), (11, 22));
    assert_eq!(history.list(1).await.unwrap()[0].id, "reflection-id");
}

#[tokio::test]
async fn malformed_provider_text_returns_safe_runtime_error_and_memory_records_parse_failure() {
    let secret = "SECRET-provider-raw-response";
    let provider = StaticProvider {
        response: secret.to_string(),
    };
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let model = model();
    let error = execute_reflection(
        &[],
        "en",
        false,
        ReflectionInvocation {
            provider: &provider,
            model: &model,
            max_tokens: 4_096,
            requested_reasoning: share::reasoning::ReasoningLevel::Off,
            system_prompt_text: "system",
        },
        &NoOpMemory,
        &history,
        &identity(),
        &cancel,
    )
    .await
    .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::Unparseable);
    assert!(!error.to_string().contains(secret));
    assert_eq!(
        history.list(1).await.unwrap()[0].error_category,
        Some(ReflectionErrorCategory::Parse)
    );
}

// ─── 有界韧性（空响应重试 / 解析修复）脚本化 provider ───────────────

/// 单次 provider 调用的脚本步骤。
enum ScriptedStep {
    /// 正常文本响应（附带固定 usage）。
    Text(String),
    /// 流正常闭合但无任何文本（空响应）。
    Empty,
    /// `invoke` 直接返回 provider 错误。
    InvokeError,
}

/// 一次调用的可断言快照：system prompt 与逐条消息（角色标签 + 文本）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordedRequest {
    system: String,
    messages: Vec<(&'static str, String)>,
}

/// 按调用顺序回放响应的反思测试 provider。脚本耗尽后再次被调用即 panic——
/// 由此断言重试与修复的调用次数有界（超出上限的第 N+1 次调用必现失败）。
struct ScriptedReflectionProvider {
    steps: Mutex<VecDeque<ScriptedStep>>,
    requests: Mutex<Vec<RecordedRequest>>,
}

impl ScriptedReflectionProvider {
    fn new(steps: Vec<ScriptedStep>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl ProviderPort for ScriptedReflectionProvider {
    async fn invoke(
        &self,
        request: ProviderRequestData,
        _cancel: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<ProviderResponseStream, ProviderError> {
        let messages = request
            .messages
            .iter()
            .map(|message| (role_label(&message.role), message.text_content()))
            .collect();
        self.requests.lock().unwrap().push(RecordedRequest {
            system: request.system.clone(),
            messages,
        });
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("反思 provider 调用超出脚本：重试与修复的调用次数 MUST 有界");
        match step {
            ScriptedStep::Text(text) => Ok(text_completion_stream(text, 11, 22)),
            ScriptedStep::Empty => Ok(text_completion_stream("", 0, 0)),
            ScriptedStep::InvokeError => Err(ProviderError::fatal(
                provider::ProviderErrorKind::UpstreamUnavailable,
                "scripted provider failure",
            )),
        }
    }
}

fn role_label(role: &share::message::Role) -> &'static str {
    match role {
        share::message::Role::User => "user",
        share::message::Role::Assistant => "assistant",
    }
}

async fn run_scripted_reflection(
    provider: &ScriptedReflectionProvider,
    history: &RecordingHistory,
    lang: &str,
    cancel: &tokio_util::sync::CancellationToken,
) -> ReflectionExecutionResultType<CompleteReflectionResult> {
    let model = model();
    execute_reflection(
        &[share::message::Message::user("reflect")],
        lang,
        false,
        ReflectionInvocation {
            provider,
            model: &model,
            max_tokens: 4_096,
            requested_reasoning: share::reasoning::ReasoningLevel::Off,
            system_prompt_text: "system",
        },
        &NoOpMemory,
        history,
        &identity(),
        cancel,
    )
    .await
}

const VALID_REFLECTION_JSON: &str =
    r#"{"deviations":["drift"],"suggested_memories":[],"outdated_memories":[]}"#;
const REPAIRED_REFLECTION_JSON: &str =
    r#"{"deviations":["repaired"],"suggested_memories":[],"outdated_memories":[]}"#;

#[tokio::test]
async fn empty_first_response_is_retried_once_with_the_same_prompt() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Empty,
        ScriptedStep::Text(REPAIRED_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let result = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap();

    assert_eq!(result.output.deviations, ["repaired"]);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    // 重试 MUST 用同一 prompt、同一 system 原样重发。
    assert_eq!(requests[0].system, "system");
    assert_eq!(requests[0], requests[1]);
}

#[tokio::test]
async fn empty_response_twice_fails_after_exactly_one_retry() {
    let provider = ScriptedReflectionProvider::new(vec![ScriptedStep::Empty, ScriptedStep::Empty]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::EmptyResponse);
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(
        history.list(1).await.unwrap()[0].error_category,
        Some(ReflectionErrorCategory::EmptyResponse)
    );
}

#[tokio::test]
async fn provider_error_is_never_retried() {
    let provider = ScriptedReflectionProvider::new(vec![ScriptedStep::InvokeError]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::LlmCall);
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn cancelled_empty_response_is_not_retried() {
    // 第二步脚本若被消费说明发生了重试——断言经脚本耗尽 panic 兜底。
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Empty,
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();

    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::EmptyResponse);
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn unparseable_output_triggers_one_repair_and_repaired_response_completes() {
    let broken_response = r#"{"deviations": ["truncated"#.to_string();
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(broken_response.clone()),
        ScriptedStep::Text(REPAIRED_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let result = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap();

    assert_eq!(result.output.deviations, ["repaired"]);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let repair = &requests[1];
    // 修复请求：system 原样沿用；消息 = 原反思 prompt → 原始响应 → 纠错指令。
    assert_eq!(repair.system, "system");
    assert_eq!(repair.messages.len(), 3);
    assert_eq!(repair.messages[0].0, "user");
    assert_eq!(repair.messages[0].1, requests[0].messages[0].1);
    assert_eq!(repair.messages[1].0, "assistant");
    assert_eq!(repair.messages[1].1, broken_response);
    assert_eq!(repair.messages[2].0, "user");
    let instruction = &repair.messages[2].1;
    assert!(
        instruction.contains("Output only a single JSON object"),
        "instruction = {instruction}"
    );
    assert!(
        instruction.contains("Validation error: reflection response JSON is invalid"),
        "instruction = {instruction}"
    );
}

#[tokio::test]
async fn repair_instruction_follows_zh_lang_wording() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text("这根本不是 JSON".to_string()),
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    run_scripted_reflection(&provider, &history, "zh", &cancel)
        .await
        .unwrap();

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let instruction = &requests[1].messages[2].1;
    assert!(
        instruction.contains("不是合法 JSON"),
        "instruction = {instruction}"
    );
    assert!(
        instruction.contains("校验错误：reflection response could not be parsed as JSON"),
        "instruction = {instruction}"
    );
}

#[tokio::test]
async fn failed_repair_falls_back_to_the_original_response() {
    // 原始响应可解析出 InvalidSuggestion；修复响应是垃圾文本（Unparseable）。
    // 若修复响应被交给 complete，错误应为 Unparseable——用错误种类证明回退。
    let invalid_suggestion = r#"{"deviations":[],"suggested_memories":[{"layer":"project","category":"fact","content":""}],"outdated_memories":[]}"#;
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(invalid_suggestion.to_string()),
        ScriptedStep::Text("still not json".to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::InvalidSuggestion);
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(
        history.list(1).await.unwrap()[0].error_category,
        Some(ReflectionErrorCategory::InvalidSuggestion)
    );
    // 纠错指令 MUST 携带精确校验错误摘要。
    let instruction = &provider.requests()[1].messages[2].1;
    assert!(
        instruction.contains("suggested_memories[0].content must not be empty"),
        "instruction = {instruction}"
    );
}

#[tokio::test]
async fn repair_call_provider_error_falls_back_to_original_parse_failure() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text("not json at all".to_string()),
        ScriptedStep::InvokeError,
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();

    // 修复调用的 provider 错误 NEVER 升级为 LlmCall 失败，按现状落 Parse。
    assert_eq!(error, ReflectionExecutionError::Unparseable);
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(
        history.list(1).await.unwrap()[0].error_category,
        Some(ReflectionErrorCategory::Parse)
    );
}

#[tokio::test]
async fn valid_output_calls_provider_once_without_repair() {
    let provider = ScriptedReflectionProvider::new(vec![ScriptedStep::Text(
        VALID_REFLECTION_JSON.to_string(),
    )]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let result = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap();

    assert_eq!(result.output.deviations, ["drift"]);
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn cancelled_parse_failure_skips_the_repair_call() {
    // 第二步脚本若被消费说明取消后仍发起了修复。
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text("not json at all".to_string()),
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();

    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::Unparseable);
    assert_eq!(provider.requests().len(), 1);
}
