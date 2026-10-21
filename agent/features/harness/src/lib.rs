//! Harness 组件树注册表（#2014）。
//!
//! 为 8 个可编辑组件提供统一读写面：`ComponentRegistry` 端口 +
//! 文件系统 adapter。编辑契约 omitted-is-inherited 与
//! 「校验失败整份作废」语义见 `domain::changeset`。
//! 设计契约：`docs/design/02-modules/harness/01-component-tree.md`。

pub mod adapters;
pub mod application;
pub mod constants;
pub mod domain;

pub(crate) use constants::LOG_TARGET;

pub use application::ComponentRegistry;
pub use domain::changeset::{apply_change_set, AppliedEdit, ChangeSet, ComponentEdit};
pub use domain::component::{
    diff_contents, ComponentContent, ComponentId, ComponentKind, ComponentName, ContentChange,
};
pub use domain::errors::HarnessError;
