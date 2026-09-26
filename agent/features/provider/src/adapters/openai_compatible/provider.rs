use crate::adapters::client::OpenAIProviderConfig;

use super::driver::{driver_for_provider_driver, ChatApiDriver};
use super::ReasoningConfig;

pub struct OpenAICompatibleProvider {
    pub(super) config: OpenAIProviderConfig,
    pub(super) api_key: String,
    pub(super) base_url: String,
    pub(super) model: String,
    pub(super) user_agent: String,
    pub(super) http: reqwest::Client,
    pub(super) reasoning_config: Option<ReasoningConfig>,
    pub(super) driver: Box<dyn ChatApiDriver + Send + Sync>,
}

impl OpenAICompatibleProvider {
    #[allow(clippy::too_many_arguments, dead_code)]
    pub fn new(
        config: OpenAIProviderConfig,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
        reasoning_config: Option<ReasoningConfig>,
        timeout_secs: u64,
    ) -> Self {
        Self::new_with_user_agent(
            config,
            api_key,
            base_url,
            model,
            max_tokens,
            reasoning,
            reasoning_config,
            timeout_secs,
            share::config::Config::default().api.user_agent,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_user_agent(
        config: OpenAIProviderConfig,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        _max_tokens: u32,
        _reasoning: bool,
        reasoning_config: Option<ReasoningConfig>,
        // 历史 signature 兼容占位：timeout 不进入本 driver 的 client 构造。
        _timeout_secs: u64,
        user_agent: String,
    ) -> Self {
        let http = crate::adapters::transport::build_http_client_for_endpoint(Some(
            base_url.as_deref().unwrap_or("https://api.openai.com"),
        ));
        Self::from_shared_http(
            config,
            api_key,
            base_url,
            model,
            reasoning_config,
            user_agent,
            http,
        )
    }

    /// 基于共享（pool 复用的）HTTP client 构造 driver；连接事实由
    /// `ProviderTransport` 持有，本结构只保存引用与展示字段。
    pub(crate) fn from_shared_http(
        config: OpenAIProviderConfig,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        reasoning_config: Option<ReasoningConfig>,
        user_agent: String,
        http: reqwest::Client,
    ) -> Self {
        let driver = driver_for_provider_driver(config.driver);
        let raw_base_url = base_url.expect("Provider construction 必须传入已解析 base URL");
        let model = model.expect("Provider construction 必须传入已解析模型");
        let base_url = if matches!(
            config.driver,
            crate::ProviderDriverKind::Minimax
                | crate::ProviderDriverKind::Mimo
                | crate::ProviderDriverKind::Agnes
        ) {
            raw_base_url.trim_end_matches('/').to_string()
        } else {
            raw_base_url
                .trim_end_matches('/')
                .trim_end_matches("/v1")
                .to_string()
        };
        Self {
            base_url,
            model,
            config,
            api_key,
            user_agent,
            http,
            reasoning_config,
            driver,
        }
    }

    pub(crate) fn chat_url(&self) -> String {
        format!("{}{}", self.base_url, self.config.chat_api_suffix)
    }
}
