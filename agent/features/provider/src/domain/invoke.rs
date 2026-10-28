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
        // #2148：server_effort_map 是服务端归并权威（#2032），允许把 Off 归并为
        // 更高档（如 Zhipu off→low）；effective 高于 requested 是合法形态，
        // 不再视为配置错误。model 非空与 max_tokens>0 校验保留。
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
    fn invocation_scope_allows_effective_reasoning_above_requested() {
        // #2148：server_effort_map 是服务端归并权威（#2032），可把请求档位
        // 归并为更高档（如 Zhipu off→low）；effective 高于 requested 合法。
        assert!(InvocationScopeData::new(
            "claude-sonnet",
            4096,
            ReasoningLevel::Low,
            ReasoningLevel::High,
        )
        .is_ok());
    }
}
