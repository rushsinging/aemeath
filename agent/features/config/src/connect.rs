//! Config-owned Connect 向导状态机。
//!
//! ## 入口
//!
//! - [`ConnectAppService`]：服务端状态拥有者，提供 `start_connect` / `apply` /
//!   `cancel` / `view`；
//! - [`ConnectView`]：单次响应的只读 DTO，绝不含 API key 明文；
//! - [`ConnectCommand`]：类型化命令枚举，必须携带 `expected_revision`；
//! - [`ConnectError`]：稳定错误分类（`InvalidTransition` / `StaleRevision` /
//!   `Validation` / `CatalogUnavailable` / `ProbeFailed` /
//!   `PersistConflict` / `PersistFailed` / `PersistUnavailable` /
//!   `InteractiveSetupRequired` / `BootstrapRollbackRefused`）；
//! - [`ProviderProbePort`] / [`ConnectCommitPort`]：inject 端口，便于
//!   Composition 装配与测试 mock。
//!
//! 设计文档：[`docs/design/02-modules/config/02-provider-catalog-and-connect.md`](../../../../../../docs/design/02-modules/config/02-provider-catalog-and-connect.md)。

#[path = "core/connect/command.rs"]
mod command;
#[path = "core/connect/commit.rs"]
mod commit;
#[path = "core/connect/draft.rs"]
mod draft;
#[path = "core/connect/error.rs"]
mod error;
#[path = "core/connect/outcome.rs"]
mod outcome;
#[path = "core/connect/service.rs"]
mod service;
#[path = "core/connect/states.rs"]
mod states;
#[path = "core/connect/view.rs"]
mod view;

#[cfg(test)]
#[path = "core/connect/service_tests.rs"]
mod service_tests;

// 重新导出 ——
pub use command::ConnectCommand;
pub use commit::{
    ConnectCommitError, ConnectCommitPort, ConnectCommitReceipt, ConnectCommitRequest,
};
pub use draft::{ConnectDraft, DraftValidationError, ModelDraft};
pub use error::{ConnectError, PersistErrorKind};
pub use outcome::ConnectOutcome;
pub use service::{ConnectAppService, ConnectAppServiceBuilder};
pub use states::{
    ConnectOrigin, ConnectRevision, ConnectSessionId, ConnectStage, DriverIdOrString,
    ExistingCredentialStatus, ExistingProviderSnapshot,
};
pub use view::{
    AvailableAction, ConnectDraftView, ConnectView, ExistingProviderSummary, ModelDraftView,
    ProbeStatusView,
};

// -- ports crate 的 re-export，让 connect_tests 直接用 `crate::connect::*` --
//
// `ProviderProbePort` 与 request/result/error 在 `crate::ports` 中定义，
// 这里是 connect 的稳定 re-export 入口；测试代码可直接 `use crate::connect::*`。
pub use crate::ports::{
    ProviderProbeError, ProviderProbeErrorKind, ProviderProbePort, ProviderProbeRequest,
    ProviderProbeResult,
};
