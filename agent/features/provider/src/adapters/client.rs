//! Unified LLM client that supports multiple providers

use std::sync::Arc;

use crate::adapters::openai_compatible::ReasoningConfig;
use crate::domain::capability::ReasoningLevel;
use crate::ports::LlmProvider;
use crate::ProviderDriverKind;
use share::message::Message;
use tokio_util::sync::CancellationToken;

fn reasoning_level_from_options(
    reasoning: bool,
    config: Option<&ReasoningConfig>,
) -> crate::domain::capability::ReasoningLevel {
    match config {
        Some(ReasoningConfig::Object(value)) => value
            .get("effort")
            .or_else(|| value.get("reasoning_effort"))
            .and_then(|value| value.as_str())
            .and_then(crate::domain::capability::ReasoningLevel::parse)
            .unwrap_or(if reasoning {
                crate::domain::capability::ReasoningLevel::High
            } else {
                crate::domain::capability::ReasoningLevel::Off
            }),
        Some(ReasoningConfig::ThinkingBudget(tokens)) => {
            if *tokens == 0 {
                crate::domain::capability::ReasoningLevel::Off
            } else {
                crate::domain::capability::ReasoningLevel::High
            }
        }
        Some(ReasoningConfig::Bool(enabled)) => {
            if *enabled {
                crate::domain::capability::ReasoningLevel::High
            } else {
                crate::domain::capability::ReasoningLevel::Off
            }
        }
        None if reasoning => crate::domain::capability::ReasoningLevel::High,
        None => crate::domain::capability::ReasoningLevel::Off,
    }
}

/// 校验 invocation 构造输入已完成 Config 解析；base URL / 模型 / UA
/// 任一缺失时 fail-closed 返回 Configuration 错误，禁止回落 adapter 内置默认值。
fn ensure_resolved_invocation_inputs(
    options: &ProviderClientSpecData,
) -> Result<(), crate::LlmError> {
    if options
        .base_url
        .as_deref()
        .is_none_or(|base_url| base_url.trim().is_empty())
    {
        return Err(crate::LlmError::Config(
            "Provider base URL 未解析：调用方必须传入 Config Catalog 或用户配置值".to_string(),
        ));
    }
    if options.model.trim().is_empty() {
        return Err(crate::LlmError::Config(
            "Provider 模型未解析：调用方必须传入 Config Catalog 或用户配置值".to_string(),
        ));
    }
    if options
        .user_agent
        .as_deref()
        .is_none_or(|user_agent| user_agent.trim().is_empty())
    {
        return Err(crate::LlmError::Config(
            "Provider User-Agent 未解析：调用方必须传入 Config resolver 的最终值".to_string(),
        ));
    }
    Ok(())
}

/// 解析 driver 字符串与 API style；无效输入显式报 Configuration 错误。
fn parse_driver_spec(
    options: &ProviderClientSpecData,
) -> Result<crate::domain::driver_acl::DriverSpec, crate::LlmError> {
    crate::domain::driver_acl::DriverSpec::parse(&options.driver, options.api_style.as_deref())
        .map_err(|error| crate::LlmError::Config(error.to_string()))
}

/// 从构造配置推导 transport key；model / max_tokens / reasoning 是
/// invocation 配置，MUST NOT 进入 key。
fn transport_key_for(
    options: &ProviderClientSpecData,
    spec: &crate::domain::driver_acl::DriverSpec,
) -> crate::adapters::transport::TransportKey {
    crate::adapters::transport::TransportKey {
        driver_kind: spec.kind(),
        api_style: options.api_style.clone(),
        base_url: options.base_url.clone(),
        api_key: options.api_key.clone(),
        // 与 driver 构造相同的 user-agent 解析，保证 key 语义一致。
        user_agent: options
            .user_agent
            .clone()
            .unwrap_or_else(|| share::config::Config::default().api.user_agent),
        timeout_secs: options.timeout_secs,
    }
}

/// 按 protocol family 构造 driver（注入给定 HTTP client）与默认 scope；
/// `from_config` 与 `from_config_with_pool` 的共用实现。
fn build_provider_and_scope(
    spec: crate::domain::driver_acl::DriverSpec,
    options: ProviderClientSpecData,
    http: reqwest::Client,
) -> Result<
    (
        Arc<dyn LlmProvider>,
        crate::domain::invoke::InvocationScopeData,
    ),
    crate::ProviderError,
> {
    use crate::domain::driver_acl::{ApiStyle, ProtocolFamily};

    let driver = spec.kind();
    let requested_reasoning =
        reasoning_level_from_options(options.reasoning, options.reasoning_config.as_ref());
    let model = options.model.clone();
    let resolved_user_agent = options
        .user_agent
        .unwrap_or_else(|| share::config::Config::default().api.user_agent);
    let provider_impl: Arc<dyn LlmProvider> = match spec.family() {
        ProtocolFamily::AnthropicMessages => {
            Arc::new(crate::adapters::AnthropicProvider::from_shared_http(
                options.api_key,
                options.base_url,
                Some(options.model),
                options.timeout_secs,
                resolved_user_agent,
                http,
            ))
        }
        ProtocolFamily::OllamaNative => {
            Arc::new(crate::adapters::OllamaProvider::from_shared_http(
                options.api_key,
                options.base_url,
                Some(options.model),
                options.timeout_secs,
                resolved_user_agent,
                http,
            ))
        }
        ProtocolFamily::OpenAi(api_style) => {
            let config = OpenAIProviderConfig::from_driver(driver, &options.source_key)
                .with_responses_api(api_style == ApiStyle::Responses);
            Arc::new(crate::adapters::OpenAICompatibleProvider::from_shared_http(
                config,
                options.api_key,
                options.base_url,
                Some(options.model),
                options.reasoning_config,
                resolved_user_agent,
                http,
            ))
        }
    };
    let effective_reasoning = requested_reasoning.clamped_to(provider_impl.max_reasoning_level());
    let default_scope = crate::domain::invoke::InvocationScopeData::new(
        model,
        if options.max_tokens == 0 {
            share::config::models::DEFAULT_MAX_TOKENS
        } else {
            options.max_tokens
        },
        requested_reasoning,
        effective_reasoning,
    )?;
    Ok((provider_impl, default_scope))
}

/// 客户端装配内核（`wire_provider_client` 内部复用；探测走独立
/// transport 的 `wire_probe_client`）。
pub(crate) fn assemble_client(
    options: ProviderClientSpecData,
    pool: &crate::adapters::pool::TransportPool,
    default_reasoning: ReasoningLevel,
) -> Result<Arc<LlmClient>, crate::ProviderError> {
    let client = LlmClient::from_config_with_pool(options, pool)?;
    let client = client.with_default_reasoning(default_reasoning)?;
    Ok(Arc::new(client))
}

/// factory build 的 provider 侧装配单入口（#1861 v4：构造配置 + 模型元数据
/// → `(client, 修正版 ModelInfo)`；原装配句柄包消除）。
///
/// `model` 携带身份、supports_* 与调用限制（组合根从 config/catalog 投影
/// 构造）；`supported_reasoning` 阶梯由 client 推导覆盖（provider 读侧权威
/// ——阶梯取决于 driver 能力，装配时才可知）。跨 BC 的 spec→config 翻译与
/// binding 组装留在组合根桥接层（binding.requested_reasoning 由 spec 直取，
/// 与输入重复的 wiring.requested_reasoning 随句柄包一并消除）。
pub fn wire_provider_client(
    config: ProviderClientSpecData,
    model: crate::published_language::ModelInfo,
    pool: &crate::adapters::pool::TransportPool,
) -> Result<(Arc<LlmClient>, crate::published_language::ModelInfo), crate::ProviderError> {
    // 构造面内核的默认推理档位由 config 自身推导（bool/reasoning_config）；
    // binding 级 requested_reasoning 由组合根从 spec 直取，不经此处。
    let default_reasoning =
        reasoning_level_from_options(config.reasoning, config.reasoning_config.as_ref());
    let client = assemble_client(config, pool, default_reasoning)?;
    let model = crate::published_language::ModelInfo {
        supported_reasoning: crate::domain::capability::supported_reasoning_from_max(
            client.max_reasoning_level(),
        ),
        ..model
    };
    Ok((client, model))
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "client_invoke_tests.rs"]
mod invoke_tests;

/// Truncate a string to at most `max_bytes`, snapping to the nearest char boundary.
fn truncate_preview(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let end = s.floor_char_boundary(max_bytes);
    format!("{}…", &s[..end]) // allow unsafe_text_op: floor_char_boundary derived end
}

/// Configuration for OpenAI-compatible providers. The source key is used only
/// for display/logging; API behavior comes from `driver`.
#[derive(Debug, Clone)]
pub(crate) struct OpenAIProviderConfig {
    pub source_key: String,
    pub driver: ProviderDriverKind,
    pub chat_api_suffix: String,
    /// 是否使用 Responses API（/v1/responses）替代 Chat Completions。
    pub use_responses_api: bool,
}

impl OpenAIProviderConfig {
    pub(crate) fn from_driver(driver: ProviderDriverKind, source_key: &str) -> Self {
        Self {
            source_key: source_key.to_string(),
            driver,
            chat_api_suffix: match driver {
                ProviderDriverKind::Zhipu => "/chat/completions".to_string(),
                ProviderDriverKind::Anthropic => "/v1/messages".to_string(),
                ProviderDriverKind::Volcengine => "/chat/completions".to_string(),
                ProviderDriverKind::Minimax => "/chat/completions".to_string(),
                ProviderDriverKind::Mimo => "/chat/completions".to_string(),
                ProviderDriverKind::DeepSeek => "/chat/completions".to_string(),
                ProviderDriverKind::Agnes => "/chat/completions".to_string(),
                // Ollama 有专用 OllamaProvider，不经此 OpenAI 兼容路径；
                // 兜底归入 OpenAI 风格 suffix。
                ProviderDriverKind::OpenAI
                | ProviderDriverKind::LiteLLM
                | ProviderDriverKind::Ollama => "/v1/chat/completions".to_string(),
            },
            use_responses_api: false,
        }
    }

    pub(crate) fn with_responses_api(mut self, enabled: bool) -> Self {
        self.use_responses_api = enabled;
        self
    }
}

pub struct ProviderClientSpecData {
    pub driver: String,
    pub source_key: String,
    pub api_style: Option<String>,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub reasoning_config: Option<ReasoningConfig>,
    pub timeout_secs: u64,
    pub user_agent: Option<String>,
}

pub struct LlmClient {
    provider: Arc<dyn LlmProvider>,
    default_scope: crate::domain::invoke::InvocationScopeData,
    /// 底层共享 transport 的诊断 id；`None` 表示独占（非 pool）HTTP client。
    transport_id: Option<u64>,
}

impl LlmClient {
    #[cfg(test)]
    pub fn from_provider(provider: Arc<dyn LlmProvider>) -> Self {
        let default_scope = crate::domain::invoke::InvocationScopeData::new(
            provider.model_name(),
            share::config::models::DEFAULT_MAX_TOKENS,
            crate::domain::capability::ReasoningLevel::Off,
            crate::domain::capability::ReasoningLevel::Off,
        )
        .expect("provider defaults must form a valid invocation scope");
        Self {
            provider,
            default_scope,
            transport_id: None,
        }
    }
}

impl LlmClient {
    pub fn from_config(options: ProviderClientSpecData) -> Result<Self, crate::ProviderError> {
        ensure_resolved_invocation_inputs(&options)?;
        let spec = parse_driver_spec(&options)?;
        let http =
            crate::adapters::transport::build_http_client_for_endpoint(options.base_url.as_deref());
        let (provider, default_scope) = build_provider_and_scope(spec, options, http)?;
        Ok(Self {
            provider,
            default_scope,
            transport_id: None,
        })
    }

    /// 与 [`Self::from_config`] 相同，但 HTTP client 经 `TransportPool` 按
    /// transport key 复用：同 key（同 driver/endpoint/认证域/user-agent/timeout）
    /// 的多次构造共享同一连接池；model / max_tokens / reasoning 属于
    /// invocation 配置，不影响 transport 复用。
    pub fn from_config_with_pool(
        options: ProviderClientSpecData,
        pool: &crate::adapters::pool::TransportPool,
    ) -> Result<Self, crate::ProviderError> {
        ensure_resolved_invocation_inputs(&options)?;
        let spec = parse_driver_spec(&options)?;
        let key = transport_key_for(&options, &spec);
        let transport = pool.acquire(key);
        let (provider, default_scope) =
            build_provider_and_scope(spec, options, transport.http().clone())?;
        Ok(Self {
            provider,
            default_scope,
            transport_id: Some(transport.id()),
        })
    }

    /// 直调 driver 流（probe 与集成测试用；invoke 是生产唯一入口）。
    pub(crate) async fn invocation_stream(
        &self,
        resolved: &crate::ports::ResolvedInvocation,
        system: &str,
        static_prefix_len: usize,
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::ProviderResponseStream, crate::ProviderError> {
        self.provider
            .invocation_stream(
                resolved,
                system,
                static_prefix_len,
                messages,
                tool_schemas,
                cancel,
            )
            .await
    }

    /// 非流式请求入口（#1880 v3 单通道输出）：经 [`Self::invoke_stream`]
    /// 建立流后 drain 全部片段，聚合为 [`crate::ProviderResponse`]（自带
    /// 成败——无需 Result 双通道）。
    ///
    /// 聚合规则：
    /// - `Content`：终态内容块（Text/Thinking/ToolCall 与 `ToolCallCompleted`
    ///   携带的完整调用）累计进 `output`；流式增量帧
    ///   （`ToolCallStarted`/`ToolArgumentsDelta`）不进终态输出；
    /// - `Usage` → `token_usage`；`Stop` → `stop_reason`；
    /// - `Error`：失败终止帧 → `ok=false` + `error=Some` 并提前返回；
    /// - 流耗尽未见 `Stop`：协议错误 → `ok=false`。
    ///
    /// establishment 失败仍走 `Err(ProviderError)`（与流式路径一致）。
    pub async fn invoke(
        &self,
        model: &crate::ModelInfo,
        request: &crate::ProviderRequestData,
    ) -> Result<crate::ProviderResponse, crate::ProviderError> {
        use crate::published_language::{ProviderContentData, ProviderResponseChunk};
        use crate::ProviderError;
        use futures_util::StreamExt;

        // 与 invoke_stream 的 resolve 同源：capability clamp 后的生效档位。
        let effective_reasoning = model.resolve_reasoning(request.reasoning);
        let mut stream = self.invoke_stream(model, request).await?;
        let mut response = crate::ProviderResponse {
            ok: false,
            error: None,
            output: Vec::new(),
            stop_reason: None,
            token_usage: None,
            effective_reasoning,
        };
        while let Some(chunk) = stream.next().await {
            match chunk {
                ProviderResponseChunk::Content(block) => match block {
                    // 流增量帧只服务渐进消费，不进终态 output。尾部 Thinking
                    // 完整帧（含 signature）后到——覆盖先前累计的同位块。
                    ProviderContentData::Text(_) | ProviderContentData::ToolCall { .. } => {
                        response.output.push(block);
                    }
                    thinking_block @ ProviderContentData::Thinking { .. } => {
                        // 尾部完整帧（含 signature）覆盖同文本的累计块。
                        let same_text = |existing: &ProviderContentData| {
                            matches!(
                                (existing, &thinking_block),
                                (
                                    ProviderContentData::Thinking { thinking: t, .. },
                                    ProviderContentData::Thinking { thinking, .. },
                                ) if t == thinking
                            )
                        };
                        response.output.retain(|existing| !same_text(existing));
                        response.output.push(thinking_block);
                    }
                    ProviderContentData::ToolCallCompleted {
                        id,
                        name,
                        arguments,
                        ..
                    } => {
                        response.output.push(ProviderContentData::ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    ProviderContentData::ToolCallStarted { .. }
                    | ProviderContentData::ToolArgumentsDelta { .. } => {}
                },
                ProviderResponseChunk::Usage(usage) => response.token_usage = Some(usage),
                ProviderResponseChunk::Stop(reason) => {
                    response.stop_reason = Some(reason);
                    response.ok = true;
                }
                ProviderResponseChunk::Error(error) => {
                    response.ok = false;
                    response.error = Some(error);
                    return Ok(response);
                }
            }
        }
        if !response.ok {
            // 流耗尽无 Stop 终止帧 = 协议错误（成功路径由 Stop 置位）。
            response.error = Some(ProviderError::fatal(
                crate::ProviderErrorKind::Protocol,
                "流结束未包含 Stop 终止帧",
            ));
        }
        Ok(response)
    }

    /// 流式请求入口：runtime PL 的 [`ProviderRequestData`] 在 crate 内
    /// 完成 reasoning clamp、scope 构造、system block / tool schema 转换与
    /// 取消竞速（原 composition ProviderAdapter 编排收编）。
    pub async fn invoke_stream(
        &self,
        model: &crate::ModelInfo,
        request: &crate::ProviderRequestData,
    ) -> Result<crate::ProviderResponseStream, crate::ProviderError> {
        use crate::ProviderError;

        // fast path：请求携带的取消 token 已触发（取消真相源在 runtime，
        // provider 只消费意图——v2 单通道）。
        if request.cancellation.is_cancelled() {
            return Err(ProviderError::cancelled());
        }

        // clamp：请求 reasoning 不超过 ModelInfo 声明能力（resolve 属 client 编排职责）。
        let requested_reasoning = request.reasoning;
        let effective_reasoning = model.resolve_reasoning(requested_reasoning);
        let resolved = crate::ports::ResolvedInvocation::new(
            request.model.clone(),
            request.max_output_tokens,
            requested_reasoning,
            effective_reasoning,
        )?;

        // request.tools 已是 wire-ready tool 定义（context::ToolSchemaData 投影产物）。
        let tool_schemas = request.tools.clone();

        log::debug!(target: crate::LOG_TARGET,
            "[LLM REQUEST] invocation params: model={} max_tokens={} requested_reasoning={:?} effective_reasoning={:?}",
            request.model, request.max_output_tokens, resolved.requested_reasoning, resolved.effective_reasoning,
        );
        self.log_request(&request.system, &request.messages, &request.tools);
        // request 携带的 token 与调用方信号竞速 establishment。
        let cancel_token = request.cancellation.clone();
        let establishment = Box::pin(self.invocation_stream(
            &resolved,
            &request.system,
            request.static_prefix_len,
            &request.messages,
            &tool_schemas,
            &cancel_token,
        ));
        tokio::pin!(establishment);

        tokio::select! {
            biased;
            _ = request.cancellation.cancelled() => {
                cancel_token.cancel();
                Err(ProviderError::cancelled())
            }
            result = &mut establishment => result,
        }
    }

    fn log_request(&self, system: &str, messages: &[Message], tool_schemas: &[serde_json::Value]) {
        if !log::log_enabled!(log::Level::Debug) {
            return;
        }
        let msg_summary: Vec<serde_json::Value> = messages.iter().enumerate().map(|(i, msg)| {
            let blocks: Vec<serde_json::Value> = msg.content.iter().map(|block| match block {
                share::message::ContentBlock::Text { text } => {
                    serde_json::json!({"type":"text","preview":truncate_preview(text,200)})
                }
                share::message::ContentBlock::ToolUse { name, input, .. } => {
                    let input_str = input.to_string();
                    serde_json::json!({"type":"tool_use","name":name,"input_preview":truncate_preview(&input_str,300)})
                }
                share::message::ContentBlock::ToolResult { content, is_error, .. } => {
                    let s = content.to_string();
                    serde_json::json!({"type":"tool_result","is_error":is_error,"preview":truncate_preview(&s,300)})
                }
                share::message::ContentBlock::Thinking { thinking, .. } => {
                    serde_json::json!({"type":"thinking","preview":truncate_preview(thinking,200)})
                }
                share::message::ContentBlock::Image { .. } => {
                    serde_json::json!({"type":"image","preview":"[image data]"})
                }
            }).collect();
            serde_json::json!({"index":i,"role":format!("{:?}",msg.role).to_lowercase(),"blocks":blocks})
        }).collect();
        let system_preview = truncate_preview(system, 200);
        // 计算 messages 总字符数用于 DEBUG 摘要
        let total_chars: usize = messages
            .iter()
            .flat_map(|m| m.content.iter())
            .map(|b| match b {
                share::message::ContentBlock::Text { text } => text.len(),
                share::message::ContentBlock::Thinking { thinking, .. } => thinking.len(),
                share::message::ContentBlock::ToolUse { input, .. } => input.to_string().len(),
                share::message::ContentBlock::ToolResult { content, .. } => {
                    content.to_string().len()
                }
                share::message::ContentBlock::Image { .. } => 0,
            })
            .sum();
        log::debug!(target: crate::LOG_TARGET,
            "[LLM REQUEST] provider={} model={} system_len={} messages={}({} chars) tools={}",
            self.provider_name(), self.model_name(), system.len(), messages.len(), total_chars, tool_schemas.len(),
        );
        log::trace!(target: crate::LOG_TARGET,
            "[LLM REQUEST] system: {:?}\n  messages: {}",
            system_preview, serde_json::to_string_pretty(&msg_summary).unwrap_or_default(),
        );
    }

    pub fn with_default_reasoning(
        mut self,
        requested_reasoning: crate::domain::capability::ReasoningLevel,
    ) -> Result<Self, crate::ProviderError> {
        self.default_scope = crate::domain::invoke::InvocationScopeData::new(
            self.default_scope.model(),
            self.default_scope.max_tokens(),
            requested_reasoning,
            requested_reasoning.clamped_to(self.provider.max_reasoning_level()),
        )?;
        Ok(self)
    }

    pub fn default_scope(&self) -> &crate::domain::invoke::InvocationScopeData {
        &self.default_scope
    }

    /// 底层共享 transport 的诊断 id；`None` 表示独占（非 pool）HTTP client。
    /// 用于日志与契约测试断言"同 key 复用同一 transport 真相"。
    pub fn transport_id(&self) -> Option<u64> {
        self.transport_id
    }

    pub fn invocation_scope(
        &self,
        model: impl Into<String>,
        max_tokens: Option<u32>,
        requested_reasoning: crate::domain::capability::ReasoningLevel,
    ) -> Result<crate::domain::invoke::InvocationScopeData, crate::ProviderError> {
        crate::domain::invoke::InvocationScopeData::new(
            model,
            max_tokens.unwrap_or_else(|| self.default_scope.max_tokens()),
            requested_reasoning,
            requested_reasoning.clamped_to(self.provider.max_reasoning_level()),
        )
    }

    pub fn model_name(&self) -> &str {
        self.provider.model_name()
    }
    pub fn provider_name(&self) -> &str {
        self.provider.provider_name()
    }
    pub fn max_reasoning_level(&self) -> crate::domain::capability::ReasoningLevel {
        self.provider.max_reasoning_level()
    }
}
