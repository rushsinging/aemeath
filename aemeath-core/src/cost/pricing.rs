//! 模型定价配置

use serde::{Deserialize, Serialize};

/// 模型定价配置（每百万 token）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    /// 模型名称
    pub model: String,
    /// 输入 token 每百万价格（USD）
    pub input_price_per_m: f64,
    /// 输出 token 每百万价格（USD）
    pub output_price_per_m: f64,
    /// 缓存写入每百万价格（USD）
    pub cache_write_price_per_m: Option<f64>,
    /// 缓存读取每百万价格（USD）
    pub cache_read_price_per_m: Option<f64>,
}

impl ModelPricing {
    /// 计算给定 token 用量的费用
    pub fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_price_per_m;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_price_per_m;
        input_cost + output_cost
    }

    /// 计算包含缓存 token 的费用
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

/// 默认模型定价
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
        // Claude Sonnet 3.5 v1
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

/// 获取指定模型的定价
pub fn get_pricing(model: &str) -> ModelPricing {
    let pricing_list = default_pricing();

    for pricing in &pricing_list {
        if pricing.model == model {
            return pricing.clone();
        }
    }

    let base_name = model.split('-').take(3).collect::<Vec<_>>().join("-");
    for pricing in &pricing_list {
        if pricing.model.starts_with(&base_name) {
            return pricing.clone();
        }
    }

    ModelPricing {
        model: model.to_string(),
        input_price_per_m: 3.0,
        output_price_per_m: 15.0,
        cache_write_price_per_m: Some(3.75),
        cache_read_price_per_m: Some(0.3),
    }
}

/// 格式化 token 数量，使用 k/m 后缀
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
