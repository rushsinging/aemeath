use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct RoleConfig {
    pub name: String,
    pub description: String,
    pub pool_size: usize,
    pub system_prompt: String,
    pub skills: Vec<String>,
    pub models: Vec<RoleModelConfig>,
    pub permissions: RolePermissions,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct RoleModelConfig {
    pub model: String,
    pub cost_tier: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct RolePermissions {
    pub allowed_tools: Vec<String>,
    pub scope: Vec<String>,
    pub max_subagents: usize,
    pub can_call_roles: Vec<String>,
    pub can_create_agents: bool,
}

pub fn parse_role_config(content: &str) -> Result<RoleConfig, toml::de::Error> {
    toml::from_str(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_role_config_reads_flat_fields_and_permissions() {
        let config = parse_role_config(
            r#"
name = "scheduler"
description = "管理 Agent Pool 生命周期"
pool_size = 0
system_prompt = "调度 Agent"
skills = []

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = []
scope = ["agent_registry", "board_read", "board_write"]
max_subagents = 0
can_call_roles = ["assistant", "executor"]
can_create_agents = true
"#,
        )
        .expect("role config parses");

        assert_eq!(config.name, "scheduler");
        assert_eq!(config.models[0].cost_tier, "low");
        assert!(config.permissions.can_create_agents);
        assert_eq!(
            config.permissions.can_call_roles,
            vec!["assistant", "executor"]
        );
    }
}
