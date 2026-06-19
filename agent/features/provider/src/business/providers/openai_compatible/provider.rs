use crate::core::client::OpenAIProviderConfig;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use super::driver::{driver_for_provider_driver, ChatApiDriver};
use super::ReasoningConfig;

pub struct OpenAICompatibleProvider {
    pub(super) config: OpenAIProviderConfig,
    pub(super) api_key: String,
    pub(super) base_url: String,
    pub(super) model: String,
    pub(super) max_tokens: Arc<AtomicU32>,
    pub(super) user_agent: String,
    pub(super) http: reqwest::Client,
    pub(super) max_retries: u32,
    pub(super) reasoning: Arc<std::sync::atomic::AtomicBool>,
    pub(super) reasoning_config: Arc<Mutex<Option<ReasoningConfig>>>,
    pub(super) driver: Box<dyn ChatApiDriver + Send + Sync>,
}

pub(crate) fn build_streaming_http_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
}

impl OpenAICompatibleProvider {
    pub fn new(
        config: OpenAIProviderConfig,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
        reasoning_config: Option<ReasoningConfig>,
    ) -> Self {
        let driver = driver_for_provider_driver(config.driver);
        let raw_base_url = base_url.unwrap_or_else(|| "https://api.openai.com".to_string());
        let base_url = if matches!(
            config.driver,
            crate::api::ProviderDriverKind::Minimax | crate::api::ProviderDriverKind::Mimo
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
            max_tokens: Arc::new(AtomicU32::new(max_tokens)),
            user_agent: format!("aemeath/{}", share::version()),
            http: build_streaming_http_client_builder()
                .build()
                .expect("failed to create HTTP client"),
            max_retries: 10,
            reasoning: Arc::new(std::sync::atomic::AtomicBool::new(reasoning)),
            reasoning_config: Arc::new(Mutex::new(reasoning_config)),
            driver,
        }
    }

    pub(crate) fn chat_url(&self) -> String {
        format!("{}{}", self.base_url, self.config.chat_api_suffix)
    }

    pub(crate) fn current_max_tokens(&self) -> u32 {
        self.max_tokens.load(Ordering::Relaxed)
    }
}
