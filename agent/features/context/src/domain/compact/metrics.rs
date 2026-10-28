//! 压缩管线观测指标（#2132）。
//!
//! 字段语义与 `[compact] ...` 日志一一对应：形状（分块/并发/波次）、
//! 阶段耗时（map/reduce/refresh/总）、降级计数。日志负责事后排查，
//! 本结构负责结构化消费（评测 harness / 事件），两条路径同一真相源。

/// 单次压缩的观测指标；`Default` 表示"未采集"。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactMetricsData {
    /// 实际调用的压缩模型（`provider/model`）；本地压缩为 `None`。
    pub model: Option<String>,
    /// 压缩模型输出上限。
    pub max_output_tokens: Option<u32>,
    /// 压缩前消息总数。
    pub total_messages: usize,
    /// 参与摘要的早期消息数。
    pub early_messages: usize,
    /// 参与摘要的早期消息 token 估算。
    pub early_tokens: usize,
    /// map 分块数（未走 map-reduce 时为 0）。
    pub chunk_count: usize,
    /// map 并发上限。
    pub concurrency: usize,
    /// map 波次 = ceil(chunk_count / concurrency)。
    pub waves: usize,
    /// 各块耗时之和：用于计算并行效率，识别"串行等待"。
    pub chunk_ms_sum: u64,
    /// 单块耗时 p50。
    pub chunk_ms_p50: u64,
    /// 单块耗时 p95。
    pub chunk_ms_p95: u64,
    /// 最慢单块耗时。
    pub chunk_ms_max: u64,
    /// map 阶段墙钟耗时。
    pub map_ms: u64,
    /// reduce 阶段耗时（本地归并 + 渲染）。
    pub reduce_ms: u64,
    /// refresh 收敛轮数。
    pub refresh_rounds: usize,
    /// refresh 累计耗时。
    pub refresh_ms: u64,
    /// 降级到本地摘要的块数。
    pub degraded_chunks: usize,
    /// 压缩整体墙钟耗时。
    pub total_ms: u64,
}

impl CompactMetricsData {
    /// 并行效率：各块耗时之和 / map 墙钟（理想值≈`concurrency`）。
    ///
    /// 明显低于并发上限说明 map 实际是串行等待（provider 限流/单连接），
    /// 而不是模型本身慢。
    pub fn parallel_efficiency(&self) -> f64 {
        if self.map_ms == 0 {
            0.0
        } else {
            self.chunk_ms_sum as f64 / self.map_ms as f64
        }
    }

    /// 压缩率：摘要 token / 早期消息 token（>1 表示摘要反而变长）。
    pub fn compression_ratio(&self, summary_tokens: usize) -> f64 {
        if self.early_tokens == 0 {
            0.0
        } else {
            summary_tokens as f64 / self.early_tokens as f64
        }
    }
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod metrics_tests;
