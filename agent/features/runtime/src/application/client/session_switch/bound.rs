//! shared permit 绑定的 Main Run 四元组快照。
//!
//! 平移自 context `application/main_session.rs` 的 `BoundMainRun`（#2025）：
//! permit 生命周期内阻塞 resume（exclusive），四字段同一瞬间捕获、
//! Run 全程一致。

use std::sync::Arc;

use context::session::CanonicalSession;
use memory::api::MemoryPort;
use share::config::domain::snapshot::ConfigSnapshot;

use super::gate::OwnedSessionSharedPermit;

pub struct BoundMainRun {
    /// 持有期内阻塞 resume（exclusive）；release 解锁，不可读取。
    permit: OwnedSessionSharedPermit,
    session: Arc<CanonicalSession>,
    context: context::ContextPorts,
    memory: Arc<dyn MemoryPort>,
    config: ConfigSnapshot,
}

impl BoundMainRun {
    pub(super) fn new(
        permit: OwnedSessionSharedPermit,
        session: Arc<CanonicalSession>,
        context: context::ContextPorts,
        memory: Arc<dyn MemoryPort>,
        config: ConfigSnapshot,
    ) -> Self {
        Self {
            permit,
            session,
            context,
            memory,
            config,
        }
    }

    /// 释放 shared permit，显式结束绑定（resume 解锁点）。
    pub fn release(self) {
        let Self {
            permit,
            session: _,
            context: _,
            memory: _,
            config: _,
        } = self;
        drop(permit);
    }

    pub fn session(&self) -> &CanonicalSession {
        &self.session
    }

    pub fn session_arc(&self) -> Arc<CanonicalSession> {
        Arc::clone(&self.session)
    }

    pub fn context(&self) -> &context::ContextPorts {
        &self.context
    }

    pub fn memory(&self) -> &dyn MemoryPort {
        self.memory.as_ref()
    }

    pub fn memory_arc(&self) -> Arc<dyn MemoryPort> {
        Arc::clone(&self.memory)
    }

    pub fn config(&self) -> &ConfigSnapshot {
        &self.config
    }
}
