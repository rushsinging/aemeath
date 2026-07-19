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

pub(crate) fn build_streaming_http_client_builder(_timeout_secs: u64) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(crate::CONNECT_TIMEOUT_SECS))
}

impl OpenAICompatibleProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: OpenAIProviderConfig,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        _max_tokens: u32,
        _reasoning: bool,
        reasoning_config: Option<ReasoningConfig>,
        timeout_secs: u64,
    ) -> Self {
        let driver = driver_for_provider_driver(config.driver);
        let raw_base_url = base_url.unwrap_or_else(|| "https://api.openai.com".to_string());
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
            model: model.unwrap_or_else(|| "gpt-4o".to_string()),
            config,
            api_key,
            user_agent: format!("aemeath/{}", share::version()),
            http: build_streaming_http_client_builder(timeout_secs)
                .build()
                .expect("failed to create HTTP client"),
            reasoning_config,
            driver,
        }
    }

    pub(crate) fn chat_url(&self) -> String {
        format!("{}{}", self.base_url, self.config.chat_api_suffix)
    }
}
