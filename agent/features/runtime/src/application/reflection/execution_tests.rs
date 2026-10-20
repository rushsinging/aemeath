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
        150,
        200,
        0,
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
    // 两步：Step1 与 Step2 各计一次。
    assert_eq!((result.input_tokens, result.output_tokens), (22, 44));
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
        150,
        200,
        0,
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
    reasoning: share::reasoning::ReasoningLevel,
    max_output_tokens: u32,
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
            reasoning: request.reasoning,
            max_output_tokens: request.max_output_tokens,
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
        150,
        200,
        0,
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
/// 两步反思 Step 1（去噪）的空产出响应：多数既有脚本的脚本头。
const NOISE_EMPTY_JSON: &str = r#"{"outdated_memories":[]}"#;
const NOISE_STALE_JSON: &str = r#"{"outdated_memories":["M1"]}"#;
const REPAIRED_REFLECTION_JSON: &str =
    r#"{"deviations":["repaired"],"suggested_memories":[],"outdated_memories":[]}"#;

#[tokio::test]
async fn noise_parse_failure_repairs_once_then_continues_to_synthesis() {
    let broken_response = r#"{"outdated_memories":[{"id":"M1"}]}"#;
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(broken_response.to_string()),
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let result = run_scripted_reflection(&provider, &history, "zh", &cancel)
        .await
        .unwrap();
    assert_eq!(result.output.deviations, ["drift"]);
    assert_eq!((result.input_tokens, result.output_tokens), (33, 66));
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    let repair = &requests[1];
    assert_eq!(repair.messages.len(), 3);
    assert_eq!(repair.messages[0], requests[0].messages[0]);
    assert_eq!(
        repair.messages[1],
        ("assistant", broken_response.to_string())
    );
    assert!(repair.messages[2].1.contains("校验错误"));
    assert!(
        requests[2].messages[0]
            .1
            .contains("Recent conversation summary")
            || requests[2].messages[0].1.contains("最近对话摘要")
    );
    for request in requests {
        assert_eq!(request.system, "system");
        assert_eq!(request.reasoning, share::reasoning::ReasoningLevel::Off);
        assert_eq!(request.max_output_tokens, 4_096);
    }
    assert_eq!(
        history.list(1).await.unwrap()[0].status,
        memory::api::reflection::ReflectionStatus::Succeeded
    );
}

#[tokio::test]
async fn noise_failed_repair_remains_bounded_and_records_original_parse_failure() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text("not json".to_string()),
        ScriptedStep::Text("still not json".to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();
    assert_eq!(error, ReflectionExecutionError::Unparseable);
    assert_eq!(provider.requests().len(), 2, "去噪只允许一次格式修复");
    assert_eq!(
        history.list(1).await.unwrap()[0].error_category,
        Some(ReflectionErrorCategory::Parse)
    );
}

#[tokio::test]
async fn noise_repair_provider_error_keeps_original_parse_failure() {
    for repair in [ScriptedStep::InvokeError, ScriptedStep::Empty] {
        let provider = ScriptedReflectionProvider::new(vec![
            ScriptedStep::Text("not json".to_string()),
            repair,
        ]);
        let history = RecordingHistory::default();
        let cancel = tokio_util::sync::CancellationToken::new();
        let error = run_scripted_reflection(&provider, &history, "en", &cancel)
            .await
            .unwrap_err();
        assert_eq!(error, ReflectionExecutionError::Unparseable);
        assert_eq!(provider.requests().len(), 2);
        assert_eq!(
            history.list(1).await.unwrap()[0].error_category,
            Some(ReflectionErrorCategory::Parse)
        );
    }
}

#[tokio::test]
async fn noise_empty_response_retries_before_the_single_parse_repair() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Empty,
        ScriptedStep::Text("not json".to_string()),
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let result = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap();
    assert_eq!(result.output.deviations, ["drift"]);
    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(requests[2].messages.len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn parse_diagnostic_records_phase_position_and_masked_preview() {
    let _capture = crate::test_log::begin();
    let private_text = "sk-private-memory-token";
    let broken_response =
        format!(r#"{{"outdated_memories":["{private_text}",{{"id":"{private_text}"}}]}}"#);
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(broken_response),
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap();
    let records = crate::test_log::drain();
    let diagnostic = records
        .iter()
        .find(|(_, message)| message.contains("reflection_parse_diagnostic"))
        .expect("解析失败必须输出诊断");
    assert_eq!(diagnostic.0, log::Level::Debug);
    for expected in [
        "phase=noise",
        "attempt=initial",
        "category=Data",
        "line=1",
        "column=",
        "response_chars=",
        "response_preview=",
    ] {
        assert!(diagnostic.1.contains(expected), "{}", diagnostic.1);
    }
    for (_, message) in records {
        assert!(!message.contains(private_text), "日志不能包含私有响应正文");
    }
}

#[tokio::test]
async fn noise_empty_response_retry_is_bounded() {
    let provider = ScriptedReflectionProvider::new(vec![ScriptedStep::Empty, ScriptedStep::Empty]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();
    assert_eq!(error, ReflectionExecutionError::EmptyResponse);
    assert_eq!(provider.requests().len(), 2);
}

#[tokio::test]
async fn empty_first_response_is_retried_once_with_the_same_prompt() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
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
    assert_eq!(requests.len(), 3);
    // 重试 MUST 用同一 prompt、同一 system 原样重发（[0]=Step1 去噪）。
    assert_eq!(requests[1].system, "system");
    assert_eq!(requests[1], requests[2]);
}

#[tokio::test]
async fn empty_response_twice_fails_after_exactly_one_retry() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
        ScriptedStep::Empty,
        ScriptedStep::Empty,
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::EmptyResponse);
    assert_eq!(provider.requests().len(), 3);
    assert_eq!(
        history.list(1).await.unwrap()[0].error_category,
        Some(ReflectionErrorCategory::EmptyResponse)
    );
}

#[tokio::test]
async fn provider_error_is_never_retried() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
        ScriptedStep::InvokeError,
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::LlmCall);
    assert_eq!(provider.requests().len(), 2);
}

#[tokio::test]
async fn cancelled_empty_response_is_not_retried() {
    // 取消短路 Step1（脚本无去噪步）；第二步脚本若被消费说明发生了重试——
    // 断言经脚本耗尽 panic 兜底。
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
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
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
    assert_eq!(requests.len(), 3);
    let repair = &requests[2];
    // 修复请求：system 原样沿用；消息 = 原反思 prompt → 原始响应 → 纠错指令。
    assert_eq!(repair.system, "system");
    assert_eq!(repair.messages.len(), 3);
    assert_eq!(repair.messages[0].0, "user");
    assert_eq!(repair.messages[0].1, requests[1].messages[0].1);
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
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
        ScriptedStep::Text("这根本不是 JSON".to_string()),
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    run_scripted_reflection(&provider, &history, "zh", &cancel)
        .await
        .unwrap();

    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    let instruction = &requests[2].messages[2].1;
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
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
        ScriptedStep::Text(invalid_suggestion.to_string()),
        ScriptedStep::Text("still not json".to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let error = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::InvalidSuggestion);
    assert_eq!(provider.requests().len(), 3);
    assert_eq!(
        history.list(1).await.unwrap()[0].error_category,
        Some(ReflectionErrorCategory::InvalidSuggestion)
    );
    // 纠错指令 MUST 携带精确校验错误摘要。
    let instruction = &provider.requests()[2].messages[2].1;
    assert!(
        instruction.contains("suggested_memories[0].content must not be empty"),
        "instruction = {instruction}"
    );
}

#[tokio::test]
async fn repair_call_provider_error_falls_back_to_original_parse_failure() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
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
    assert_eq!(provider.requests().len(), 3);
    assert_eq!(
        history.list(1).await.unwrap()[0].error_category,
        Some(ReflectionErrorCategory::Parse)
    );
}

#[tokio::test]
async fn valid_output_calls_provider_once_without_repair() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();

    let result = run_scripted_reflection(&provider, &history, "en", &cancel)
        .await
        .unwrap();

    assert_eq!(result.output.deviations, ["drift"]);
    assert_eq!(provider.requests().len(), 2);
}

#[tokio::test]
async fn reflection_execution_preserves_request_parameters_during_retry_and_repair() {
    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(NOISE_EMPTY_JSON.to_string()),
        ScriptedStep::Empty,
        ScriptedStep::Text("not json at all".to_string()),
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let model = model();
    let result = execute_reflection(
        &[share::message::Message::user("reflect")],
        "en",
        false,
        150,
        200,
        0,
        ReflectionInvocation {
            provider: &provider,
            model: &model,
            max_tokens: 64_000,
            requested_reasoning: share::reasoning::ReasoningLevel::High,
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
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        4,
        "去噪、归纳、空响应重试和解析修复必须完整执行"
    );
    for request in requests {
        assert_eq!(request.reasoning, share::reasoning::ReasoningLevel::High);
        assert_eq!(request.max_output_tokens, 64_000);
    }
}

#[tokio::test]
async fn cancelled_parse_failure_skips_the_repair_call() {
    // 取消短路 Step1（脚本无去噪步）；第二步脚本若被消费说明取消后仍发起了修复。
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

/// #1987 两步反思：Step1 去噪真实落盘（under_permit），Step2 在干净盘面上
/// 完成；usage 为两步合计。
#[tokio::test]
async fn two_phase_reflection_removes_noise_then_completes() {
    use memory::api::{InMemoryMemory, MemoryLayer, MemoryPort as _};

    let memory = InMemoryMemory::new(memory::api::MemoryPolicy {
        max_entries: 10,
        similarity_threshold: 0.8,
        target_active_entries: 7,
        default_ttl_secs: 0,
    })
    .unwrap();
    let noisy = memory::api::MemoryEntry::new(
        memory::api::MemoryId::now_v7(),
        100,
        MemoryLayer::Project,
        memory::api::MemoryCategory::Fact,
        "#1234 已 squash 合入 main：PR #1235 → abc1234，分支已清理",
        memory::api::MemorySource::User,
    )
    .unwrap();
    memory.write(noisy.clone()).await.unwrap();
    // 残留：独立 mark_outdated 不归档的旧数据，等编排 sweep 收口。
    let residual = memory::api::MemoryEntry::new(
        memory::api::MemoryId::now_v7(),
        100,
        MemoryLayer::Project,
        memory::api::MemoryCategory::Fact,
        "独立的过时残留条目",
        memory::api::MemorySource::User,
    )
    .unwrap();
    memory.write(residual.clone()).await.unwrap();
    assert!(memory.mark_outdated(&residual.id).await.unwrap());

    let provider = ScriptedReflectionProvider::new(vec![
        ScriptedStep::Text(NOISE_STALE_JSON.to_string()),
        ScriptedStep::Text(VALID_REFLECTION_JSON.to_string()),
    ]);
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let model = model();
    let invocation = ReflectionInvocation {
        provider: &provider,
        model: &model,
        max_tokens: 4_096,
        requested_reasoning: share::reasoning::ReasoningLevel::Off,
        system_prompt_text: "system",
    };

    let result = execute_reflection(
        &[share::message::Message::user("reflect")],
        "en",
        true,
        7,
        10,
        0,
        invocation,
        &memory,
        &history,
        &identity(),
        &cancel,
    )
    .await
    .unwrap();

    // Sweep + Step1 淘汰落盘：残留与噪音条目都归档出 active。
    assert_eq!(memory.stats().await.project_count, 0);
    assert_eq!(memory.stats().await.project_archive_count, 2);
    // 两步各一次调用；token 合并（ScriptedProvider 每次同额）。
    assert_eq!(provider.requests().len(), 2);
    assert_eq!((result.input_tokens, result.output_tokens), (22, 44));
    let applied = result.apply_result.as_ref().expect("必须汇总去噪应用结果");
    assert_eq!(applied.attempted, 1);
    assert_eq!(applied.completed, 1);
    assert_eq!(applied.outdated_marked, 1);
    let persisted = history.records.lock().unwrap();
    let record = persisted.last().expect("最终记录必须落盘");
    assert_eq!(record.apply_result.as_ref(), Some(applied));
    // sweep 归档的失效残留计数进 record（本例：residual 1 条；noisy 走去噪不计入）。
    assert_eq!(record.stale_swept, Some(1));
    drop(persisted);
    // Step1 的 Step2 prompt 不再含噪音条目。
    let second_prompt = &provider.requests()[1];
    assert!(
        !second_prompt
            .messages
            .iter()
            .any(|(_, text)| text.contains("abc1234")),
        "Step2 prompt MUST NOT 含已清除条目"
    );
}
