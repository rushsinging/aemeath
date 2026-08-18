//! Connect 提交的窄 seam。
//!
//! 本 Task **不**实现 filesystem / CAS / global store；`ConnectCommitPort`
//! 是一个适配器契约，由 Composition 在后续 Task 注入 `GlobalConfigConnectStore`
//! adapter 实现。当前阶段由测试 mock 提供。
//!
//! ## 设计约束
//!
//! - 输入仅包含 [`crate::connect::ConnectDraft`] 投影 + session 上下文；
//! - Adapter 实现**禁止**写入 ConfigWriter 的 project override 路径
//!   （`ConfigWriter::update` 是另一条 seam）；
//! - 返回 typed [`ConnectCommitError`]，禁止向上抛出未分类 IO/serde 错误；
//! - Adapter 必须先调用 global document schema 校验再做 CAS；
//! - 失败分两类：CAS 冲突（`PersistConflict`）与其他稳定错误（`PersistFailed`）。

use async_trait::async_trait;

use crate::catalog::ProviderSource;

use super::draft::ConnectDraft;
use super::error::PersistErrorKind;
use super::states::{ConnectOrigin, ConnectSessionId};
use crate::GlobalConfigRevision;

/// Connect 提交请求的归一化投影。
#[derive(Debug, Clone)]
pub struct ConnectCommitRequest {
    pub session_id: ConnectSessionId,
    pub origin: ConnectOrigin,
    pub expected_global_revision: GlobalConfigRevision,
    pub draft: ConnectDraft,
}

impl ConnectCommitRequest {
    /// 从 draft 派生稳定 `ProviderSource`：本字段由 service 在写完 draft 后
    /// 拷贝出来，避免 adapter 在 fs 错误路径上回头访问 draft。
    pub fn source(&self) -> Option<ProviderSource> {
        self.draft.source
    }
}

/// Connect 提交成功回执。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectCommitReceipt {
    /// adapter 写入后能读到的新 revision。下游 SDK / CLI 应把该值作为下次
    /// `committed_snapshot()` 的依据。
    pub applied_revision: u64,
}

/// Connect 提交 typed error。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectCommitError {
    /// CAS 失败：调用方期望的 revision 与当前不同；UI 引导用户重载。
    PersistConflict { expected: u64 },
    /// 非冲突的写入 / 校验失败。
    PersistFailed {
        kind: PersistErrorKind,
        message: String,
    },
    /// Adapter 没被注入或不可用；service 会返回 ConnectError::PersistUnavailable。
    PersistUnavailable,
}

impl ConnectCommitError {
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::PersistFailed {
            kind: PersistErrorKind::Serialization,
            message: message.into(),
        }
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::PersistFailed {
            kind: PersistErrorKind::Io,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::PersistFailed {
            kind: PersistErrorKind::Internal,
            message: message.into(),
        }
    }
}

/// Connect 提交端口契约。
///
/// 实现方职责：
/// - 校验 [`ConnectCommitRequest`] 的稳定字段（source / driver / base URL /
///   context_window / max_tokens）；
/// - 在 CAS 成功后把 receipt 透传给 service；
/// - 失败时按类别映射为 [`ConnectCommitError`]；
/// - **NEVER** 隐式读取环境变量、shell、本地真实路径；
/// - **NEVER** 调用 `ConfigWriter::update`（那是另一条 seam）。
#[async_trait]
pub trait ConnectCommitPort: Send + Sync {
    async fn commit(
        &self,
        request: ConnectCommitRequest,
    ) -> Result<ConnectCommitReceipt, ConnectCommitError>;
}

/// 测试 / 桩实现：直接返回 caller 配置的结果。**仅**在 `connect::commit`
/// 模块内可见（pub(super)），不进入生产 API。
#[cfg(test)]
pub mod test_helpers {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// 可变 commit 结果的 helper，供 connect_tests 编排失败注入。
    pub struct StubCommitPort {
        pub outcome: Mutex<CommitOutcome>,
        pub requests: Mutex<Vec<ConnectCommitRequest>>,
    }

    #[derive(Debug, Clone)]
    pub enum CommitOutcome {
        Success { applied_revision: u64 },
        Failure(ConnectCommitError),
    }

    impl StubCommitPort {
        pub fn new(outcome: CommitOutcome) -> Arc<Self> {
            Arc::new(Self {
                outcome: Mutex::new(outcome),
                requests: Mutex::new(Vec::new()),
            })
        }

        pub async fn set_outcome(&self, outcome: CommitOutcome) {
            *self.outcome.lock().await = outcome;
        }
    }

    #[async_trait]
    impl ConnectCommitPort for StubCommitPort {
        async fn commit(
            &self,
            request: ConnectCommitRequest,
        ) -> Result<ConnectCommitReceipt, ConnectCommitError> {
            self.requests.lock().await.push(request.clone());
            let outcome = self.outcome.lock().await.clone();
            match outcome {
                CommitOutcome::Success { applied_revision } => {
                    Ok(ConnectCommitReceipt { applied_revision })
                }
                CommitOutcome::Failure(err) => Err(err),
            }
        }
    }
}
