use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ToolDisplay, ToolDisplayEntry, ToolRenderPolicy,
};
use std::path::Path;

struct SkillDisplay;

impl ToolDisplay for SkillDisplay {
    fn name(&self) -> &str {
        "Skill"
    }

    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let identity = input
            .get("skill")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("?");
        format!("Skill {identity}")
    }

    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        Vec::new()
    }

    fn header_for_subagent(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> String {
        self.format_header(input, workspace_root)
    }

    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden,
        }
    }
}

inventory::submit!(ToolDisplayEntry {
    name: "Skill",
    display: || Box::new(SkillDisplay),
});

#[cfg(test)]
#[path = "skill_tests.rs"]
mod tests;
