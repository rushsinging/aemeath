use serde::{Deserialize, Serialize};

const fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    #[serde(default = "default_enabled")]
    pub snip_enabled: bool,
    #[serde(default = "default_enabled")]
    pub microcompact_enabled: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            snip_enabled: true,
            microcompact_enabled: true,
        }
    }
}
