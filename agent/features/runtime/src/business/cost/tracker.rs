//! 费用追踪器

use crate::utils::bootstrap::config_paths as paths;

use super::pricing::get_pricing;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 单条 API 使用记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// API 调用时间戳
    pub timestamp: u64,
    /// 会话 ID
    pub session_id: String,
    /// 使用的模型
    pub model: String,
    /// 输入 token 数
    pub input_tokens: u32,
    /// 输出 token 数
    pub output_tokens: u32,
    /// 缓存写入 token 数
    pub cache_write_tokens: Option<u32>,
    /// 缓存读取 token 数
    pub cache_read_tokens: Option<u32>,
    /// 计算的费用（USD）
    pub cost: f64,
}

/// 费用追踪器
pub struct CostTracker {
    /// 使用记录
    records: Vec<UsageRecord>,
    /// 历史文件路径
    path: PathBuf,
}

impl CostTracker {
    /// 创建新的费用追踪器
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            path: paths::global_cost_history_path(),
        }
    }

    /// 从磁盘加载历史
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

    /// 保存历史到磁盘
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

    /// 记录一次 API 使用
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

        cost
    }

    /// 所有会话的总费用
    pub fn total_cost(&self) -> f64 {
        self.records.iter().map(|r| r.cost).sum()
    }

    /// 指定会话的总费用
    pub fn session_cost(&self, session_id: &str) -> f64 {
        self.records
            .iter()
            .filter(|r| r.session_id == session_id)
            .map(|r| r.cost)
            .sum()
    }

    /// 所有会话的总 token 数
    pub fn total_tokens(&self) -> (u32, u32) {
        let input = self.records.iter().map(|r| r.input_tokens).sum();
        let output = self.records.iter().map(|r| r.output_tokens).sum();
        (input, output)
    }

    /// 指定会话的 token 数
    pub fn session_tokens(&self, session_id: &str) -> (u32, u32) {
        let input = self
            .records
            .iter()
            .filter(|r| r.session_id == session_id)
            .map(|r| r.input_tokens)
            .sum();
        let output = self
            .records
            .iter()
            .filter(|r| r.session_id == session_id)
            .map(|r| r.output_tokens)
            .sum();
        (input, output)
    }

    /// API 调用总次数
    pub fn total_calls(&self) -> usize {
        self.records.len()
    }

    /// 指定会话的 API 调用次数
    pub fn session_calls(&self, session_id: &str) -> usize {
        self.records
            .iter()
            .filter(|r| r.session_id == session_id)
            .count()
    }

    /// 获取所有记录
    pub fn records(&self) -> &[UsageRecord] {
        &self.records
    }

    /// 获取指定会话的记录
    pub fn session_records(&self, session_id: &str) -> Vec<&UsageRecord> {
        self.records
            .iter()
            .filter(|r| r.session_id == session_id)
            .collect()
    }

    /// 清除所有历史
    pub fn clear(&mut self) {
        self.records.clear();
        if let Err(e) = self.save() {
            log::warn!(target: "runtime::tracker", "Failed to save after clearing: {}", e);
        }
    }

    /// 生成总览报告
    pub fn summary(&self) -> super::summary::CostSummary {
        let total_cost = self.total_cost();
        let (total_input, total_output) = self.total_tokens();
        let total_calls = self.total_calls();

        let sessions: std::collections::HashSet<_> =
            self.records.iter().map(|r| r.session_id.clone()).collect();

        let mut model_costs: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        for record in &self.records {
            *model_costs.entry(record.model.clone()).or_insert(0.0) += record.cost;
        }

        super::summary::CostSummary {
            total_cost,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            total_calls,
            session_count: sessions.len(),
            model_costs,
        }
    }

    /// 生成会话报告
    pub fn session_summary(&self, session_id: &str) -> super::summary::SessionCostSummary {
        let cost = self.session_cost(session_id);
        let (input, output) = self.session_tokens(session_id);
        let calls = self.session_calls(session_id);

        super::summary::SessionCostSummary {
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
