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
