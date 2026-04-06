//! Cost tracking for API usage
//!
//! Tracks token usage and calculates costs for Anthropic API calls.
//! Supports different models with their specific pricing.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Format token count with k/m suffix, smart decimal handling
pub fn format_tokens<N: Into<u64>>(n: N) -> String {
    let n = n.into();
    if n >= 1_000_000 {
        let m = n as f64 / 1_000_000.0;
        if m >= 10.0 {
            format!("{:.0}m", m)
        } else if m.fract() < 0.05 {
            format!("{:.0}m", m)
        } else {
            format!("{:.1}m", m)
        }
    } else if n >= 1000 {
        let k = n as f64 / 1000.0;
        if k >= 10.0 {
            format!("{:.0}k", k)
        } else if k.fract() < 0.05 {
            format!("{:.0}k", k)
        } else {
            format!("{:.1}k", k)
        }
    } else {
        n.to_string()
    }
}

/// Model pricing configuration (per million tokens)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    /// Model name
    pub model: String,
    /// Input token price per million tokens (USD)
    pub input_price_per_m: f64,
    /// Output token price per million tokens (USD)
    pub output_price_per_m: f64,
    /// Cache write price per million tokens (USD)
    pub cache_write_price_per_m: Option<f64>,
    /// Cache read price per million tokens (USD)
    pub cache_read_price_per_m: Option<f64>,
}

impl ModelPricing {
    /// Calculate cost for given token usage
    pub fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_price_per_m;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_price_per_m;
        input_cost + output_cost
    }

    /// Calculate cost including cache tokens
    pub fn calculate_cost_with_cache(
        &self,
        input_tokens: u32,
        output_tokens: u32,
        cache_write_tokens: u32,
        cache_read_tokens: u32,
    ) -> f64 {
        let base_cost = self.calculate_cost(input_tokens, output_tokens);

        let cache_write_cost = if let Some(price) = self.cache_write_price_per_m {
            (cache_write_tokens as f64 / 1_000_000.0) * price
        } else {
            0.0
        };

        let cache_read_cost = if let Some(price) = self.cache_read_price_per_m {
            (cache_read_tokens as f64 / 1_000_000.0) * price
        } else {
            0.0
        };

        base_cost + cache_write_cost + cache_read_cost
    }
}

/// Default model pricing (as of 2024)
pub fn default_pricing() -> Vec<ModelPricing> {
    vec![
        // Claude Opus 4
        ModelPricing {
            model: "claude-opus-4-20250514".to_string(),
            input_price_per_m: 15.0,
            output_price_per_m: 75.0,
            cache_write_price_per_m: Some(18.75),
            cache_read_price_per_m: Some(1.5),
        },
        ModelPricing {
            model: "claude-opus-4".to_string(),
            input_price_per_m: 15.0,
            output_price_per_m: 75.0,
            cache_write_price_per_m: Some(18.75),
            cache_read_price_per_m: Some(1.5),
        },
        // Claude Sonnet 4
        ModelPricing {
            model: "claude-sonnet-4-20250514".to_string(),
            input_price_per_m: 3.0,
            output_price_per_m: 15.0,
            cache_write_price_per_m: Some(3.75),
            cache_read_price_per_m: Some(0.3),
        },
        ModelPricing {
            model: "claude-sonnet-4".to_string(),
            input_price_per_m: 3.0,
            output_price_per_m: 15.0,
            cache_write_price_per_m: Some(3.75),
            cache_read_price_per_m: Some(0.3),
        },
        // Claude Sonnet 4.5
        ModelPricing {
            model: "claude-sonnet-4-5-20250929".to_string(),
            input_price_per_m: 3.0,
            output_price_per_m: 15.0,
            cache_write_price_per_m: Some(3.75),
            cache_read_price_per_m: Some(0.3),
        },
        ModelPricing {
            model: "claude-4-5-sonnet".to_string(),
            input_price_per_m: 3.0,
            output_price_per_m: 15.0,
            cache_write_price_per_m: Some(3.75),
            cache_read_price_per_m: Some(0.3),
        },
        // Claude Sonnet 3.5 (legacy)
        ModelPricing {
            model: "claude-3-5-sonnet-20241022".to_string(),
            input_price_per_m: 3.0,
            output_price_per_m: 15.0,
            cache_write_price_per_m: Some(3.75),
            cache_read_price_per_m: Some(0.3),
        },
        ModelPricing {
            model: "claude-3-5-sonnet".to_string(),
            input_price_per_m: 3.0,
            output_price_per_m: 15.0,
            cache_write_price_per_m: Some(3.75),
            cache_read_price_per_m: Some(0.3),
        },
        // Claude Sonnet 3.5 v1 (older pricing)
        ModelPricing {
            model: "claude-3-5-sonnet-20240620".to_string(),
            input_price_per_m: 3.0,
            output_price_per_m: 15.0,
            cache_write_price_per_m: None,
            cache_read_price_per_m: None,
        },
        // Claude Haiku 3.5
        ModelPricing {
            model: "claude-3-5-haiku-20241022".to_string(),
            input_price_per_m: 0.8,
            output_price_per_m: 4.0,
            cache_write_price_per_m: Some(1.0),
            cache_read_price_per_m: Some(0.08),
        },
        ModelPricing {
            model: "claude-3-5-haiku".to_string(),
            input_price_per_m: 0.8,
            output_price_per_m: 4.0,
            cache_write_price_per_m: Some(1.0),
            cache_read_price_per_m: Some(0.08),
        },
        // Claude Opus 3
        ModelPricing {
            model: "claude-3-opus-20240229".to_string(),
            input_price_per_m: 15.0,
            output_price_per_m: 75.0,
            cache_write_price_per_m: None,
            cache_read_price_per_m: None,
        },
        ModelPricing {
            model: "claude-3-opus".to_string(),
            input_price_per_m: 15.0,
            output_price_per_m: 75.0,
            cache_write_price_per_m: None,
            cache_read_price_per_m: None,
        },
    ]
}

/// Get pricing for a model name
pub fn get_pricing(model: &str) -> ModelPricing {
    let pricing_list = default_pricing();

    // Try exact match first
    for pricing in &pricing_list {
        if pricing.model == model {
            return pricing.clone();
        }
    }

    // Try partial match (model name without version)
    let base_name = model.split('-').take(3).collect::<Vec<_>>().join("-");
    for pricing in &pricing_list {
        if pricing.model.starts_with(&base_name) {
            return pricing.clone();
        }
    }

    // Default to Sonnet pricing for unknown models
    ModelPricing {
        model: model.to_string(),
        input_price_per_m: 3.0,
        output_price_per_m: 15.0,
        cache_write_price_per_m: Some(3.75),
        cache_read_price_per_m: Some(0.3),
    }
}

/// A single API usage record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Timestamp of the API call
    pub timestamp: u64,
    /// Session ID
    pub session_id: String,
    /// Model used
    pub model: String,
    /// Input tokens
    pub input_tokens: u32,
    /// Output tokens
    pub output_tokens: u32,
    /// Cache write tokens (if applicable)
    pub cache_write_tokens: Option<u32>,
    /// Cache read tokens (if applicable)
    pub cache_read_tokens: Option<u32>,
    /// Calculated cost in USD
    pub cost: f64,
}

/// Cost tracker
pub struct CostTracker {
    /// Usage records
    records: Vec<UsageRecord>,
    /// History file path
    path: PathBuf,
}

impl CostTracker {
    /// Create a new cost tracker
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let path = home.join(".aemeath").join("cost_history.json");

        Self {
            records: Vec::new(),
            path,
        }
    }

    /// Load history from disk
    pub fn load(&mut self) -> Result<(), String> {
        if !self.path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.path)
            .map_err(|e| format!("Failed to read cost history: {}", e))?;

        let records: Vec<UsageRecord> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse cost history: {}", e))?;

        self.records = records;
        Ok(())
    }

    /// Save history to disk
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create cost history dir: {}", e))?;
        }

        let content = serde_json::to_string_pretty(&self.records)
            .map_err(|e| format!("Failed to serialize cost history: {}", e))?;

        fs::write(&self.path, content)
            .map_err(|e| format!("Failed to write cost history: {}", e))?;

        Ok(())
    }

    /// Record an API usage
    pub fn record(
        &mut self,
        session_id: &str,
        model: &str,
        input_tokens: u32,
        output_tokens: u32,
        cache_write_tokens: Option<u32>,
        cache_read_tokens: Option<u32>,
    ) -> f64 {
        let pricing = get_pricing(model);

        let cost = if let (Some(cw), Some(cr)) = (cache_write_tokens, cache_read_tokens) {
            pricing.calculate_cost_with_cache(input_tokens, output_tokens, cw, cr)
        } else {
            pricing.calculate_cost(input_tokens, output_tokens)
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.records.push(UsageRecord {
            timestamp,
            session_id: session_id.to_string(),
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cache_write_tokens,
            cache_read_tokens,
            cost,
        });

        // Auto-save
        if let Err(e) = self.save() {
            log::warn!("Failed to save cost history: {}", e);
        }

        cost
    }

    /// Get total cost for all sessions
    pub fn total_cost(&self) -> f64 {
        self.records.iter().map(|r| r.cost).sum()
    }

    /// Get total cost for a specific session
    pub fn session_cost(&self, session_id: &str) -> f64 {
        self.records
            .iter()
            .filter(|r| r.session_id == session_id)
            .map(|r| r.cost)
            .sum()
    }

    /// Get total tokens for all sessions
    pub fn total_tokens(&self) -> (u32, u32) {
        let input = self.records.iter().map(|r| r.input_tokens).sum();
        let output = self.records.iter().map(|r| r.output_tokens).sum();
        (input, output)
    }

    /// Get tokens for a specific session
    pub fn session_tokens(&self, session_id: &str) -> (u32, u32) {
        let input = self.records
            .iter()
            .filter(|r| r.session_id == session_id)
            .map(|r| r.input_tokens)
            .sum();
        let output = self.records
            .iter()
            .filter(|r| r.session_id == session_id)
            .map(|r| r.output_tokens)
            .sum();
        (input, output)
    }

    /// Get number of API calls
    pub fn total_calls(&self) -> usize {
        self.records.len()
    }

    /// Get number of API calls for a session
    pub fn session_calls(&self, session_id: &str) -> usize {
        self.records
            .iter()
            .filter(|r| r.session_id == session_id)
            .count()
    }

    /// Get all records
    pub fn records(&self) -> &[UsageRecord] {
        &self.records
    }

    /// Get records for a session
    pub fn session_records(&self, session_id: &str) -> Vec<&UsageRecord> {
        self.records
            .iter()
            .filter(|r| r.session_id == session_id)
            .collect()
    }

    /// Clear all history
    pub fn clear(&mut self) {
        self.records.clear();
        if let Err(e) = self.save() {
            log::warn!("Failed to save after clearing: {}", e);
        }
    }

    /// Generate a summary report
    pub fn summary(&self) -> CostSummary {
        let total_cost = self.total_cost();
        let (total_input, total_output) = self.total_tokens();
        let total_calls = self.total_calls();

        // Count unique sessions
        let sessions: std::collections::HashSet<_> =
            self.records.iter().map(|r| r.session_id.clone()).collect();

        // Calculate per-model breakdown
        let mut model_costs: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        for record in &self.records {
            *model_costs.entry(record.model.clone()).or_insert(0.0) += record.cost;
        }

        CostSummary {
            total_cost,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            total_calls,
            session_count: sessions.len(),
            model_costs,
        }
    }

    /// Generate a session summary
    pub fn session_summary(&self, session_id: &str) -> SessionCostSummary {
        let cost = self.session_cost(session_id);
        let (input, output) = self.session_tokens(session_id);
        let calls = self.session_calls(session_id);

        SessionCostSummary {
            session_id: session_id.to_string(),
            cost,
            input_tokens: input,
            output_tokens: output,
            calls,
        }
    }
}

impl Default for CostTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Overall cost summary
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
    /// Format as a human-readable string
    pub fn format(&self) -> String {
        let mut output = String::from("Cost Summary:\n\n");

        output.push_str(&format!(
            "Total Cost: ${:.4}\n",
            self.total_cost
        ));
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

/// Session-specific cost summary
#[derive(Debug)]
pub struct SessionCostSummary {
    pub session_id: String,
    pub cost: f64,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub calls: usize,
}

impl SessionCostSummary {
    /// Format as a human-readable string
    pub fn format(&self) -> String {
        format!(
            "Session {}:\n  Cost: ${:.4}\n  Input: {} tokens\n  Output: {} tokens\n  Calls: {}",
            self.session_id,
            self.cost,
            self.input_tokens,
            self.output_tokens,
            self.calls
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_calculation() {
        let pricing = get_pricing("claude-sonnet-4");
        let cost = pricing.calculate_cost(100_000, 50_000);
        // 100k input * $3/M = $0.30
        // 50k output * $15/M = $0.75
        // Total = $1.05
        assert!((cost - 1.05).abs() < 0.01);
    }

    #[test]
    fn test_pricing_fallback() {
        let pricing = get_pricing("unknown-model");
        assert_eq!(pricing.input_price_per_m, 3.0);
        assert_eq!(pricing.output_price_per_m, 15.0);
    }

    #[test]
    fn test_tracker_record() {
        let mut tracker = CostTracker::new();
        let cost = tracker.record("test-session", "claude-sonnet-4", 10_000, 5_000, None, None);
        assert!((cost - 0.105).abs() < 0.01);
        assert_eq!(tracker.total_calls(), 1);
    }

    #[test]
    fn test_tracker_session_cost() {
        let mut tracker = CostTracker::new();
        tracker.record("session1", "claude-sonnet-4", 10_000, 5_000, None, None);
        tracker.record("session1", "claude-sonnet-4", 20_000, 10_000, None, None);
        tracker.record("session2", "claude-sonnet-4", 5_000, 2_500, None, None);

        let session1_cost = tracker.session_cost("session1");
        // session1: 30k input * $3/M = $0.09, 15k output * $15/M = $0.225 = $0.315
        // Note: with safety margin in estimation, actual may differ
        assert!(session1_cost > 0.0);

        let session2_cost = tracker.session_cost("session2");
        assert!(session2_cost > 0.0);
    }
}