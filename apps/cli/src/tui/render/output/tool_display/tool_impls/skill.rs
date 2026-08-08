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
mod tests {
    use super::*;

    #[test]
    fn header_uses_only_identity_and_hides_details_and_result() {
        let display = SkillDisplay;
        assert_eq!(
            display.format_header(&serde_json::json!({"skill": "release"}), None),
            "Skill release"
        );
        assert_eq!(
            display.render_policy(),
            ToolRenderPolicy {
                header: HeaderPolicy::Standard,
                details: DetailsPolicy::Hidden,
                result: ResultPolicy::Hidden,
            }
        );
        assert_eq!(
            display.header_for_subagent(
                &serde_json::json!({"skill": "superpowers:using-superpowers"}),
                None,
            ),
            "Skill superpowers:using-superpowers"
        );
        assert_eq!(
            display.format_header(&serde_json::json!({"content": "BODY_SENTINEL"}), None),
            "Skill ?"
        );
    }
}
