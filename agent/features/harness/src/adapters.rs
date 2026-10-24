//! 文件系统 adapter：组件树的磁盘读写实现（#2014 首批：overlay/skill 只读枚举等，
//! 见各实现文件）。

pub mod composite;
#[cfg(test)]
mod composite_tests;
pub mod daemon_client;
pub mod daemon_serve;
#[cfg(test)]
mod daemon_tests;
pub mod filesystem;
#[cfg(test)]
mod filesystem_tests;
pub mod plugins_registry;
#[cfg(test)]
mod plugins_registry_tests;
pub mod session_scanner;
#[cfg(test)]
mod session_scanner_tests;
pub mod skills_registry;
#[cfg(test)]
mod skills_registry_tests;
