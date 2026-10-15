//! 纯值常量（#1146 placement 归位）。

pub(crate) const BM25_B: f64 = 0.75;

pub(crate) const BM25_K1: f64 = 1.2;

pub(crate) const FACET_WEIGHT: f64 = 1.0;

pub(crate) const TAG_WEIGHT: f64 = 2.0;

pub(crate) const CONTENT_WEIGHT: f64 = 3.0;

pub(crate) const EXACT_MATCH_BOOST: f64 = 100.0;

pub(crate) const PROJECT_KEY_DOMAIN: &[u8] = b"aemeath.memory.project-key.v2\0";

/// 取代链上溯的最大步数（#1774）。合法链长不会超过 `max_entries`；
/// 超过该上限说明数据已损坏，按成环处理以收敛写入路径。
pub const MAX_SUPERSEDE_CHAIN_DEPTH: usize = 256;

/// M13 的证据下限（#1776）：少于两条来源的「归纳」等价于复制既有条目，
/// MUST NOT 产出。
pub const MIN_SYNTHESIS_EVIDENCE: usize = 2;

/// Reflect 收敛目标相对 `max_entries` 的分子（分母 4 → 0.75）。
/// 首轮与同轮补问共用；NEVER 再降比例。
pub const REFLECT_TARGET_ACTIVE_RATIO_NUM: usize = 3;
pub const REFLECT_TARGET_ACTIVE_RATIO_DEN: usize = 4;

/// `floor(max_entries × 0.75)`：成功 reflect 落盘后 project active 上限。
#[must_use]
pub const fn reflect_target_active_entries(max_entries: usize) -> usize {
    max_entries.saturating_mul(REFLECT_TARGET_ACTIVE_RATIO_NUM) / REFLECT_TARGET_ACTIVE_RATIO_DEN
}
