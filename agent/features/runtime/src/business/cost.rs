//! 费用追踪 API 使用情况

pub mod pricing;
pub mod summary;
pub mod tracker;

pub use pricing::{default_pricing, format_tokens, get_pricing, ModelPricing};
pub use summary::{CostSummary, SessionCostSummary};
pub use tracker::{CostTracker, UsageRecord};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_calculation() {
        let pricing = get_pricing("claude-sonnet-4");
        let cost = pricing.calculate_cost(100_000, 50_000);
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
        assert!(session1_cost > 0.0);

        let session2_cost = tracker.session_cost("session2");
        assert!(session2_cost > 0.0);
    }
}
