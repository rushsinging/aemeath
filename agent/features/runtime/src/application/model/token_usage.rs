pub(crate) fn normalized_total_tokens(usage: &crate::ports::RawUsageSnapshotData) -> u64 {
    usage.input_tokens.unwrap_or(0) as u64 + usage.output_tokens.unwrap_or(0) as u64
}

#[cfg(test)]
#[path = "token_usage_tests.rs"]
mod tests;
