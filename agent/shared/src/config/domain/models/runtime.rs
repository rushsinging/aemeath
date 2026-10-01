use crate::config::models::{ModelResolveError, ModelsConfig, ResolvedModel};
use std::fmt;

pub use crate::config::domain::constants::DEFAULT_MAX_TOKENS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxTokensSource {
    Cli,
    Model,
    Config,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRuntimeModel {
    resolved_model: ResolvedModel,
    max_tokens: u32,
    max_tokens_source: MaxTokensSource,
}

impl ResolvedRuntimeModel {
    pub fn new(
        resolved_model: ResolvedModel,
        max_tokens: u32,
        max_tokens_source: MaxTokensSource,
    ) -> Self {
        Self {
            resolved_model,
            max_tokens,
            max_tokens_source,
        }
    }

    pub fn resolved_model(&self) -> &ResolvedModel {
        &self.resolved_model
    }

    pub fn into_resolved_model(self) -> ResolvedModel {
        self.resolved_model
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    pub fn max_tokens_source(&self) -> MaxTokensSource {
        self.max_tokens_source
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeModelRequest<'a> {
    pub model_override: Option<&'a str>,
    pub cli_max_tokens: Option<u32>,
    pub config_max_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeModelResolutionError {
    Model(ModelResolveError),
    CliMaxTokensZero,
}

impl fmt::Display for RuntimeModelResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(err) => write!(f, "{err}"),
            Self::CliMaxTokensZero => write!(f, "max_tokens 必须大于 0"),
        }
    }
}

impl std::error::Error for RuntimeModelResolutionError {}

impl From<ModelResolveError> for RuntimeModelResolutionError {
    fn from(value: ModelResolveError) -> Self {
        Self::Model(value)
    }
}

pub struct RuntimeModelResolver;

impl RuntimeModelResolver {
    pub fn resolve(
        models: &ModelsConfig,
        request: RuntimeModelRequest<'_>,
    ) -> Result<ResolvedRuntimeModel, RuntimeModelResolutionError> {
        let resolved_model = models.select_for_run(request.model_override)?;
        Self::from_resolved_model(resolved_model, request)
    }

    pub fn from_resolved_model(
        resolved_model: ResolvedModel,
        request: RuntimeModelRequest<'_>,
    ) -> Result<ResolvedRuntimeModel, RuntimeModelResolutionError> {
        let (max_tokens, source) = resolve_max_tokens(
            request.cli_max_tokens,
            resolved_model.model.max_tokens,
            request.config_max_tokens,
        )?;
        Ok(ResolvedRuntimeModel::new(
            resolved_model,
            max_tokens,
            source,
        ))
    }
}

fn resolve_max_tokens(
    cli_max_tokens: Option<u32>,
    model_max_tokens: u32,
    config_max_tokens: Option<u32>,
) -> Result<(u32, MaxTokensSource), RuntimeModelResolutionError> {
    if let Some(cli) = cli_max_tokens {
        if cli == 0 {
            return Err(RuntimeModelResolutionError::CliMaxTokensZero);
        }
        return Ok((cli, MaxTokensSource::Cli));
    }

    if model_max_tokens > 0 {
        return Ok((model_max_tokens, MaxTokensSource::Model));
    }

    if let Some(config) = config_max_tokens.filter(|v| *v > 0) {
        return Ok((config, MaxTokensSource::Config));
    }

    Ok((DEFAULT_MAX_TOKENS, MaxTokensSource::Default))
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
