pub(crate) fn normalized_total_tokens(usage: &provider::Usage) -> u64 {
    u64::from(usage.normalized_total_tokens(0))
}

#[cfg(test)]
mod tests {
    use super::normalized_total_tokens;
    use provider::Usage;

    #[test]
    fn runtime_consumes_provider_normalized_total_without_readding_cache() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            cached_tokens: Some(80),
            cache_creation_tokens: Some(30),
            total_tokens: Some(230),
            ..Usage::default()
        };

        assert_eq!(normalized_total_tokens(&usage), 230);
    }
}
