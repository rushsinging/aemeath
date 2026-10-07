use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use memory::api::search::{MemoryRetrievalMode, MemorySearchHit};
use memory::api::{MemoryPort, MemoryQuery};

use super::constants::INJECTION_CANDIDATE_LIMIT;
use crate::domain::{ContextRequestData, SystemBlock};
use crate::ports::{ContextMemorySource, MemoryMaterialization};

/// Read-only bridge from the Memory BC retrieval port into Context system blocks.
pub(crate) struct MemoryRetrieveAdapter {
    memory: Arc<dyn MemoryPort>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl MemoryRetrieveAdapter {
    pub fn new(memory: Arc<dyn MemoryPort>) -> Self {
        Self::with_clock(memory, Arc::new(system_now))
    }

    /// 按窗口比例物化注入内容。`context_size` 来自当轮 request——预算随窗口
    /// 缩放，大窗口不再浪费空间、小窗口也不会超出比例（#1777）。
    pub async fn materialize_config(
        &self,
        config: &share::config::MemoryConfig,
        context_size: usize,
    ) -> Result<MemoryMaterialization, String> {
        let token_budget = match config.inject_token_budget {
            Some(0) => return Ok(empty_materialization()),
            Some(fixed) => fixed,
            None => crate::domain::token_budget::injection_token_budget(context_size),
        };
        if !config.enabled || token_budget == 0 {
            return Ok(empty_materialization());
        }

        let result = self
            .memory
            .retrieve_for_inject(&MemoryQuery {
                limit: INJECTION_CANDIDATE_LIMIT,
                layer: None,
                category: None,
                now: (self.now)(),
            })
            .await;

        match result.mode {
            MemoryRetrievalMode::Disabled => return Ok(empty_materialization()),
            MemoryRetrievalMode::InjectionPriority => {}
            mode => {
                return Err(format!(
                    "memory retrieval returned {mode:?}; expected InjectionPriority"
                ));
            }
        }

        // 条数上限已移除（#1777）：token 预算本身就是上限。候选仍取一个
        // 足够大的窗口，保证排序与让位顺序完整，截断交给预算。
        let candidate_count = result.hits.len();
        let hits = take_with_overlay_within_budget(result.hits, token_budget);
        let estimated_tokens = hits
            .iter()
            .map(|hit| crate::domain::token_budget::estimate_tokens(&render_memory_line(hit)))
            .sum::<usize>();
        let global_hits = hits
            .iter()
            .filter(|hit| hit.entry.layer == memory::api::MemoryLayer::Global)
            .count();
        let project_hits = hits.len().saturating_sub(global_hits);
        log::debug!(
            target: crate::LOG_TARGET,
            "memory_injection_materialized candidates={} injected={} estimated_tokens={} dropped={} token_budget={} context_size={} global_hits={} project_hits={}",
            candidate_count,
            hits.len(),
            estimated_tokens,
            candidate_count.saturating_sub(hits.len()),
            token_budget,
            context_size,
            global_hits,
            project_hits
        );
        if hits.is_empty() {
            return Ok(empty_materialization());
        }

        let content = render_memory_context(&hits);
        Ok(MemoryMaterialization {
            revision: stable_revision(&hits),
            blocks: vec![SystemBlock {
                kind: "memory_context".to_string(),
                content,
                cacheable: true,
                cache_break: false,
            }],
        })
    }

    /// Constructs the adapter with an injectable Unix-seconds clock.
    pub fn with_clock(
        memory: Arc<dyn MemoryPort>,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self { memory, now }
    }
}

fn system_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[async_trait]
impl ContextMemorySource for MemoryRetrieveAdapter {
    async fn materialize(
        &self,
        request: &ContextRequestData,
    ) -> Result<MemoryMaterialization, String> {
        self.materialize_config(request.config_snapshot.memory(), request.context_size)
            .await
    }
}

fn empty_materialization() -> MemoryMaterialization {
    MemoryMaterialization {
        blocks: Vec::new(),
        revision: 0,
    }
}

/// 覆盖式让位的注入填充（#1777）。
///
/// Memory 侧已把 `kind = Synthesized` 的结论排到全部普通条目之前，组内保持
/// `injection_score` 顺序（不改分数，只改顺序）。这里做两段预算填充：
///
/// 1. 先填结论段；
/// 2. 再用剩余预算填普通条目，但**跳过被已选结论覆盖的来源**。
///
/// 「已覆盖」在填充时计算（扫描已选结论的 `evidence`），不依赖任何写时标记。
/// 关键性质：结论若在第一段就没挤进预算，它的来源在第二段照常参与——固定
/// 降权系数会在这个场景误伤来源，让位因此必须是条件性的。
fn take_with_overlay_within_budget(
    hits: Vec<MemorySearchHit>,
    token_budget: usize,
) -> Vec<MemorySearchHit> {
    let mut used_tokens = 0usize;
    let mut selected = Vec::new();
    let mut covered = std::collections::HashSet::new();

    // 段一：结论（Memory 侧已排在前），超预算即停——被挤掉的结论不产生覆盖。
    for hit in hits
        .iter()
        .filter(|hit| hit.entry.kind == memory::api::MemoryKind::Synthesized)
        .cloned()
    {
        if !fill_within_budget(&hit, token_budget, &mut used_tokens, &mut covered) {
            break;
        }
        selected.push(hit);
    }
    // 段二：其余条目，被已选结论覆盖的来源让位。
    for hit in hits
        .into_iter()
        .filter(|hit| hit.entry.kind != memory::api::MemoryKind::Synthesized)
    {
        if covered.contains(&hit.entry.id) {
            continue;
        }
        if !fill_within_budget(&hit, token_budget, &mut used_tokens, &mut covered) {
            break;
        }
        selected.push(hit);
    }
    selected
}

/// 单条填充：预算够则计入并（若是结论）登记其覆盖的来源，返回是否入账。
fn fill_within_budget(
    hit: &MemorySearchHit,
    token_budget: usize,
    used_tokens: &mut usize,
    covered: &mut std::collections::HashSet<memory::api::MemoryId>,
) -> bool {
    let tokens = crate::domain::token_budget::estimate_tokens(&render_memory_line(hit));
    if used_tokens.saturating_add(tokens) > token_budget {
        return false;
    }
    *used_tokens = used_tokens.saturating_add(tokens);
    if hit.entry.kind == memory::api::MemoryKind::Synthesized {
        covered.extend(hit.entry.evidence.iter().copied());
    }
    true
}

fn render_memory_line(hit: &MemorySearchHit) -> String {
    let pinned = if hit.entry.pinned { "★ " } else { "" };
    format!("- {pinned}[{:?}] {}", hit.entry.category, hit.entry.content)
}

fn render_memory_context(hits: &[MemorySearchHit]) -> String {
    let lines = hits.iter().map(render_memory_line);
    format!(
        "<memory-context>\n{}\n</memory-context>",
        lines.collect::<Vec<_>>().join("\n")
    )
}

fn stable_revision(hits: &[MemorySearchHit]) -> u64 {
    // FNV-1a is deterministic across processes, unlike `DefaultHasher`.
    let mut revision = 0xcbf29ce484222325_u64;
    for hit in hits {
        for byte in format!(
            "{:?}\0{}\0{}\0",
            hit.entry.category, hit.entry.content, hit.entry.pinned
        )
        .bytes()
        {
            revision ^= u64::from(byte);
            revision = revision.wrapping_mul(0x100000001b3);
        }
    }
    revision
}

pub(crate) struct CommittedMemoryRetrieveAdapter {
    memory: Arc<std::sync::RwLock<Arc<dyn MemoryPort>>>,
}

impl CommittedMemoryRetrieveAdapter {
    pub fn new(memory: Arc<std::sync::RwLock<Arc<dyn MemoryPort>>>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl ContextMemorySource for CommittedMemoryRetrieveAdapter {
    async fn materialize(
        &self,
        request: &ContextRequestData,
    ) -> Result<MemoryMaterialization, String> {
        let memory = self
            .memory
            .read()
            .map_err(|error| error.to_string())?
            .clone();
        MemoryRetrieveAdapter::new(memory)
            .materialize(request)
            .await
    }
}

/// Sub Run 或禁用 Memory 时使用的空注入 adapter。
pub(crate) struct NoOpContextMemorySource;

#[async_trait]
impl ContextMemorySource for NoOpContextMemorySource {
    async fn materialize(
        &self,
        _request: &ContextRequestData,
    ) -> Result<MemoryMaterialization, String> {
        Ok(MemoryMaterialization {
            blocks: Vec::<SystemBlock>::new(),
            revision: 0,
        })
    }
}

#[cfg(test)]
#[path = "memory_injection_tests.rs"]
mod tests;
