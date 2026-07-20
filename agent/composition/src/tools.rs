use std::sync::Arc;

use ::tools::ToolCatalogGateway;

pub fn wire_tools() -> Arc<dyn ToolCatalogGateway> {
    ::tools::wire_tools()
}

pub fn wire_commands() -> Result<::tools::composition::CommandWiring, ::tools::CommandParseError> {
    ::tools::composition::wire_commands(Vec::new())
}

pub fn wire_commands_with_skills(
    skills: &std::collections::HashMap<String, sdk::SkillView>,
) -> Result<::tools::composition::CommandWiring, ::tools::CommandParseError> {
    let mut descriptors = Vec::new();
    for skill in skills.values() {
        let aliases = skill.aliases.iter().map(String::as_str).collect::<Vec<_>>();
        descriptors.push(::tools::CommandDescriptor::new(
            &skill.name,
            &aliases,
            skill.description.as_deref().unwrap_or("Skill prompt"),
            ::tools::CommandMechanism::PromptInjection,
            ::tools::CommandTarget::ContextManagement,
            ::tools::CommandArgumentSchema::OptionalText,
        )?);
    }
    ::tools::composition::wire_commands(descriptors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_tools_returns_callable_gateway() {
        let gateway = wire_tools();
        let registry = gateway.new_registry();

        assert!(!registry.contains("Read"));
    }
}
