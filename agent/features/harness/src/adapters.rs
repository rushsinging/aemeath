//! 文件系统 adapter：组件树的磁盘读写实现（#2014 首批：overlay/skill 只读枚举等，
//! 见各实现文件）。

pub mod filesystem;
#[cfg(test)]
mod filesystem_tests;
