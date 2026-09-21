//! Transport pool：只缓存不可变 [`ProviderTransport`]。
//!
//! pool 的构造与持有只允许 Composition Root（经 `provider::composition`
//! 暴露，由 `check-provider-construction-ownership.sh` 守卫）。pool 不缓存
//! 任何可被调用方改写的 invocation 配置；key 变化（凭证轮换、endpoint 或
//! user-agent 变更）自然产生新 transport，旧实例随最后一个 `Arc` 释放，
//! 因此 **无需主动失效**：进程退出即整体 drop。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::transport::{ProviderTransport, TransportKey};

struct PoolState {
    /// 下一个 transport 诊断 id；仅在本 pool 内单调递增。
    next_transport_id: u64,
    entries: HashMap<TransportKey, Arc<ProviderTransport>>,
}

/// 不可变 transport 的进程内缓存；命中复用、未命中新建。
///
/// 生命周期与 Composition Root 装配的 provider factory 一致（进程级）。
pub struct TransportPool {
    state: RwLock<PoolState>,
}

impl TransportPool {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(PoolState {
                next_transport_id: 0,
                entries: HashMap::new(),
            }),
        }
    }

    /// 返回 key 对应的不可变 transport；同 key 永远返回同一实例。
    pub(crate) fn acquire(&self, key: TransportKey) -> Arc<ProviderTransport> {
        // Fast path：读锁命中直接复用。
        if let Ok(state) = self.state.read() {
            if let Some(transport) = state.entries.get(&key) {
                return Arc::clone(transport);
            }
        }

        // Slow path：写锁内 double-check，防止并发重复建池。
        let mut state = self
            .state
            .write()
            .expect("transport pool lock must not be poisoned");
        if let Some(transport) = state.entries.get(&key) {
            return Arc::clone(transport);
        }
        let transport = Arc::new(ProviderTransport::new(state.next_transport_id));
        state.next_transport_id += 1;
        state.entries.insert(key, Arc::clone(&transport));
        transport
    }

    /// 当前缓存的不同 transport 数量；诊断与契约测试用（例如证明
    /// "模型切换不建立第二 transport 真相"）。
    pub fn distinct_transport_count(&self) -> usize {
        self.state
            .read()
            .map(|state| state.entries.len())
            .unwrap_or(0)
    }
}

impl Default for TransportPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
