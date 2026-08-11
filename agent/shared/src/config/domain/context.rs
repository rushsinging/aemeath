use serde::{Deserialize, Serialize};

const fn default_enabled() -> bool {
    true
}

fn default_auto_compact_failure_limit() -> u8 {
    3
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    #[serde(default = "default_enabled")]
    pub snip_enabled: bool,
    #[serde(default = "default_enabled")]
    pub microcompact_enabled: bool,
    #[serde(default = "default_auto_compact_failure_limit")]
    pub auto_compact_failure_limit: u8,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            snip_enabled: true,
            microcompact_enabled: true,
            auto_compact_failure_limit: default_auto_compact_failure_limit(),
        }
    }
}
