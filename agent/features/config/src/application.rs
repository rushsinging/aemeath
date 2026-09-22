//! 应用层 mod 根：装配 app_service 与 wiring，公开路径保持 application::* 不变。
mod app_service;
mod wiring;

pub use app_service::ConfigAppService;
pub use wiring::{
    wire_project_config, wire_project_config_with_agents_dir, wire_project_config_with_cli,
    ConfigWiring,
};

#[cfg(test)]
#[path = "application_tests.rs"]
mod tests;

#[cfg(test)]
use crate::adapters::encode_native_patch;
#[cfg(test)]
use crate::adapters::{EnvSource, NativeConfigStore};
#[cfg(test)]
use crate::domain::ConfigUpdate;
#[cfg(test)]
use crate::domain::*;
#[cfg(test)]
use share::config::domain::merge::ConfigPatch;
