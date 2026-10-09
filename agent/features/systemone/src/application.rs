//! 下载用例编排（application 层）：只依赖 ports，不读环境/配置/文件系统，
//! 不包含固定 URL；manifest 与存储、抓取、安装端口全部由构造器注入。

mod download_model;

pub use download_model::{
    DownloadOutcome, ModelDownloadError, ModelDownloadErrorKind, ModelDownloadService,
};
