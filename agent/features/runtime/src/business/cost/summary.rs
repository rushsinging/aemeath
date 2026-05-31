//! 费用报告结构体

/// 总体费用摘要
#[derive(Debug)]
pub struct CostSummary {
    pub total_cost: f64,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub total_calls: usize,
    pub session_count: usize,
    pub model_costs: std::collections::HashMap<String, f64>,
}

impl CostSummary {
    /// 格式化为可读字符串
    pub fn format(&self) -> String {
        let mut output = String::from("Cost Summary:\n\n");
        output.push_str(&format!("Total Cost: ${:.4}\n", self.total_cost));
        output.push_str(&format!(
            "Input Tokens: {} ({:.2}M)\n",
            self.total_input_tokens,
            self.total_input_tokens as f64 / 1_000_000.0
        ));
        output.push_str(&format!(
            "Output Tokens: {} ({:.2}M)\n",
            self.total_output_tokens,
            self.total_output_tokens as f64 / 1_000_000.0
        ));
        output.push_str(&format!("API Calls: {}\n", self.total_calls));
        output.push_str(&format!("Sessions: {}\n", self.session_count));

        if !self.model_costs.is_empty() {
            output.push_str("\nBy Model:\n");
            for (model, cost) in &self.model_costs {
                output.push_str(&format!("  {}: ${:.4}\n", model, cost));
            }
        }

        output
    }
}

/// 会话级别的费用摘要
#[derive(Debug)]
pub struct SessionCostSummary {
    pub session_id: String,
    pub cost: f64,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub calls: usize,
}

impl SessionCostSummary {
    /// 格式化为可读字符串
    pub fn format(&self) -> String {
        format!(
            "Session {}:\n  Cost: ${:.4}\n  Input: {} tokens\n  Output: {} tokens\n  Calls: {}",
            self.session_id, self.cost, self.input_tokens, self.output_tokens, self.calls
        )
    }
}
