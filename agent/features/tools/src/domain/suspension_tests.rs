use super::{ToolSuspension, UserInteractionSpec, UserOption, UserQuestion};

#[test]
fn tool_suspension_serde_round_trip_preserves_all_user_interaction_fields() {
    let suspension =
        ToolSuspension::UserInteraction(UserInteractionSpec::new(vec![UserQuestion::new(
            "Choose",
            vec![
                UserOption::title_only("A"),
                UserOption::new("B", Some("second".to_string())),
            ],
            true,
            false,
            Some("A".to_string()),
        )]));

    let encoded = serde_json::to_string(&suspension).expect("serialize suspension");
    let decoded: ToolSuspension = serde_json::from_str(&encoded).expect("deserialize suspension");

    assert_eq!(decoded, suspension);
}
