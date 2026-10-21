//! Session 切换门禁：shared（Run 绑定）/ exclusive（resume）两级许可。
//!
//! 平移自 context `application/main_session.rs`（#2025）：语义等价、宿主归
//! runtime——resume 编排的发起方在 runtime，门禁随协议迁移。

use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock as TokioRwLock};

#[derive(Clone)]
pub(crate) struct SessionSwitchGate {
    inner: std::sync::Arc<TokioRwLock<()>>,
}

impl Default for SessionSwitchGate {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionSwitchGate {
    pub(crate) fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(TokioRwLock::new(())),
        }
    }

    pub(crate) async fn acquire_shared(
        &self,
    ) -> Result<OwnedSessionSharedPermit, SessionSwitchClosed> {
        Ok(OwnedSessionSharedPermit(
            self.inner.clone().read_owned().await,
        ))
    }

    /// 非阻塞取 shared permit（Run 创建时的准入尝试）。
    pub(crate) fn try_acquire_shared(
        &self,
    ) -> Result<OwnedSessionSharedPermit, SessionSwitchInProgress> {
        self.inner
            .clone()
            .try_read_owned()
            .map(OwnedSessionSharedPermit)
            .map_err(|_| SessionSwitchInProgress)
    }

    pub(crate) async fn acquire_owned_exclusive(
        &self,
    ) -> Result<OwnedSessionExclusivePermit, SessionSwitchClosed> {
        Ok(OwnedSessionExclusivePermit(
            self.inner.clone().write_owned().await,
        ))
    }
}

pub struct OwnedSessionSharedPermit(#[allow(dead_code)] OwnedRwLockReadGuard<()>);

pub(crate) struct OwnedSessionExclusivePermit(#[allow(dead_code)] OwnedRwLockWriteGuard<()>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Main Session 正在切换")]
pub struct SessionSwitchInProgress;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Main Session 切换门禁已关闭")]
pub struct SessionSwitchClosed;
