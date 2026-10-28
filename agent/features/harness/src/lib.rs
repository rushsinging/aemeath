//! Harness 组件树注册表（#2014）。
//!
//! 为 8 个可编辑组件提供统一读写面：`ComponentRegistry` 端口 +
//! 文件系统 adapter。编辑契约 omitted-is-inherited 与
//! 「校验失败整份作废」语义见 `domain::changeset`。
//! 设计契约：`docs/design/02-modules/harness/01-component-tree.md`。

/// 实现密封基建（#2146 实现权治理）：crate 内共享一个 Sealed 标记。
/// `pub(crate)` 模块 + `pub` trait——crate 外不可命名（路径不可达），
/// 因此「实现权」编译期归属本 crate；seal 的 PL trait 绑定
/// `crate::sealed::Sealed` 作为超 trait。
pub(crate) mod sealed {
    pub trait Sealed {}
}

pub mod adapters;
pub mod application;
pub mod constants;
pub mod domain;

pub(crate) use constants::LOG_TARGET;

pub use application::ComponentRegistry;
pub use domain::changeset::{apply_change_set, AppliedEdit, ChangeSet, ComponentEdit};
pub use domain::component::{
    diff_contents, parse_component_id, ComponentContent, ComponentId, ComponentKind, ComponentName,
    ContentChange,
};
pub use domain::errors::HarnessError;
pub use domain::evidence::{
    aggregate_evidence, EvidenceBundle, EvidenceSignal, SessionEventRecord,
};
pub use domain::plugin::{PluginRule, PluginSeamEngine, SeamEffect, SeamOutcome};
pub use domain::stream::{
    load_stream_state, plan_evidence_stream, save_stream_state, EvidenceBlockPlan,
    EvidenceStreamState, RunRef, SessionProgress, SessionStreamEntry, StreamPlanParams,
};
pub use domain::stream_port::EvidenceStreamPort;
