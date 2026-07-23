#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigIntent {
    SetProviderModel {
        provider: Option<String>,
        model_id: Option<String>,
    },
    SetContextSize(u64),
    SetThinking(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigChange {
    ProviderModelChanged {
        provider: Option<String>,
        model_id: Option<String>,
    },
    ContextSizeChanged(u64),
    ThinkingChanged(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigProvider {
    provider: Option<String>,
    model_id: Option<String>,
    context_size: u64,
    thinking: bool,
}

impl Default for ConfigProvider {
    fn default() -> Self {
        Self {
            provider: None,
            model_id: None,
            context_size: 0,
            thinking: true,
        }
    }
}

impl ConfigProvider {
    pub(crate) fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    pub(crate) fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    pub(crate) fn context_size(&self) -> u64 {
        self.context_size
    }

    pub(crate) fn thinking(&self) -> bool {
        self.thinking
    }

    pub(crate) fn apply(&mut self, intent: ConfigIntent) -> ConfigChange {
        match intent {
            ConfigIntent::SetProviderModel { provider, model_id } => {
                self.provider = provider.clone();
                self.model_id = model_id.clone();
                ConfigChange::ProviderModelChanged { provider, model_id }
            }
            ConfigIntent::SetContextSize(context_size) => {
                self.context_size = context_size;
                ConfigChange::ContextSizeChanged(context_size)
            }
            ConfigIntent::SetThinking(thinking) => {
                self.thinking = thinking;
                ConfigChange::ThinkingChanged(thinking)
            }
        }
    }
}

#[cfg(test)]
#[path = "config_provider_tests.rs"]
mod tests;
