use crate::domain::capability::ProviderDriverKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ApiStyle {
    ChatCompletions,
    Responses,
}

impl ApiStyle {
    fn parse(value: Option<&str>) -> Result<Self, DriverConfigError> {
        match value {
            None | Some("") | Some("chat") | Some("chat-completions") => Ok(Self::ChatCompletions),
            Some(value) if value.eq_ignore_ascii_case("responses") => Ok(Self::Responses),
            Some(value) => Err(DriverConfigError::UnknownApiStyle {
                api_style: value.to_string(),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProtocolFamily {
    AnthropicMessages,
    OpenAi(ApiStyle),
    OllamaNative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DriverSpec {
    kind: ProviderDriverKind,
    family: ProtocolFamily,
}

impl DriverSpec {
    pub(crate) fn parse(driver: &str, api_style: Option<&str>) -> Result<Self, DriverConfigError> {
        let kind =
            ProviderDriverKind::parse(driver).ok_or_else(|| DriverConfigError::UnknownDriver {
                driver: driver.to_string(),
            })?;
        let api_style = ApiStyle::parse(api_style)?;
        let family = match kind {
            ProviderDriverKind::Anthropic => {
                require_chat_style(driver, api_style)?;
                ProtocolFamily::AnthropicMessages
            }
            ProviderDriverKind::Ollama => {
                require_chat_style(driver, api_style)?;
                ProtocolFamily::OllamaNative
            }
            ProviderDriverKind::OpenAI
            | ProviderDriverKind::Zhipu
            | ProviderDriverKind::LiteLLM
            | ProviderDriverKind::Volcengine
            | ProviderDriverKind::Minimax
            | ProviderDriverKind::Mimo
            | ProviderDriverKind::DeepSeek
            | ProviderDriverKind::Agnes => ProtocolFamily::OpenAi(api_style),
        };
        Ok(Self { kind, family })
    }

    pub(crate) fn kind(self) -> ProviderDriverKind {
        self.kind
    }

    pub(crate) fn family(self) -> ProtocolFamily {
        self.family
    }
}

fn require_chat_style(driver: &str, api_style: ApiStyle) -> Result<(), DriverConfigError> {
    if api_style == ApiStyle::Responses {
        return Err(DriverConfigError::UnsupportedApiStyle {
            driver: driver.to_string(),
            api_style: "responses".to_string(),
        });
    }
    Ok(())
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub(crate) enum DriverConfigError {
    #[error("未知 Provider driver：{driver}")]
    UnknownDriver { driver: String },
    #[error("未知 Provider API style：{api_style}")]
    UnknownApiStyle { api_style: String },
    #[error("Provider driver '{driver}' 不支持 API style '{api_style}'")]
    UnsupportedApiStyle { driver: String, api_style: String },
}
