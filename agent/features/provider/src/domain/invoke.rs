use super::capability::ReasoningLevel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationScopeData {
    model: String,
    max_tokens: u32,
    requested_reasoning: ReasoningLevel,
    effective_reasoning: ReasoningLevel,
}

impl InvocationScopeData {
    pub fn new(
        model: impl Into<String>,
        max_tokens: u32,
        requested_reasoning: ReasoningLevel,
        effective_reasoning: ReasoningLevel,
    ) -> Result<Self, crate::ProviderError> {
        let model = model.into();
        if model.trim().is_empty() {
            return Err(crate::ProviderError::fatal(
                crate::ProviderErrorKind::Configuration,
                "invocation model must not be empty".to_string(),
            ));
        }
        if max_tokens == 0 {
            return Err(crate::ProviderError::fatal(
                crate::ProviderErrorKind::Configuration,
                "invocation max_tokens must be greater than zero".to_string(),
            ));
        }
        if effective_reasoning > requested_reasoning {
            return Err(crate::ProviderError::fatal(
                crate::ProviderErrorKind::Configuration,
                "effective reasoning must not exceed requested reasoning".to_string(),
            ));
        }
        Ok(Self {
            model,
            max_tokens,
            requested_reasoning,
            effective_reasoning,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    pub fn requested_reasoning(&self) -> ReasoningLevel {
        self.requested_reasoning
    }

    pub fn effective_reasoning(&self) -> ReasoningLevel {
        self.effective_reasoning
    }
}

#[cfg(test)]
mod invocation_scope_tests {
    use super::*;

    #[test]
    fn invocation_scope_freezes_resolved_values() {
        let scope = InvocationScopeData::new(
            "claude-sonnet",
            4096,
            ReasoningLevel::High,
            ReasoningLevel::Medium,
        )
        .expect("valid scope");

        assert_eq!(scope.model(), "claude-sonnet");
        assert_eq!(scope.max_tokens(), 4096);
        assert_eq!(scope.requested_reasoning(), ReasoningLevel::High);
        assert_eq!(scope.effective_reasoning(), ReasoningLevel::Medium);
    }

    #[test]
    fn invocation_scope_rejects_zero_max_tokens() {
        assert!(InvocationScopeData::new(
            "claude-sonnet",
            0,
            ReasoningLevel::Off,
            ReasoningLevel::Off,
        )
        .is_err());
    }

    #[test]
    fn invocation_scope_rejects_effective_reasoning_above_requested() {
        assert!(InvocationScopeData::new(
            "claude-sonnet",
            4096,
            ReasoningLevel::Low,
            ReasoningLevel::High,
        )
        .is_err());
    }
}
