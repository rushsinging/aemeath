#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}
