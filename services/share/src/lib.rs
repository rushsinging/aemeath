//! 跨 service 公共抽象层
//!
//! share 定义 services 之间的公共接口，具体实现由各 service 提供。
//! tools 等消费者只依赖 share，不直接依赖具体 service。

pub mod skill_ops;
pub mod worktree_ops;
