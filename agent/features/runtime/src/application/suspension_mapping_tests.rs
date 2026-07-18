use super::suspension_mapping::user_interaction_items;

#[test]
fn maps_user_interaction_fields_losslessly_in_stable_order() {
    let suspension = tools::ToolSuspension::UserInteraction(tools::UserInteractionSpec::new(vec![
        tools::UserQuestion::new(
            "First",
            vec![
                tools::UserOption::title_only("alpha"),
                tools::UserOption::new("beta", Some("second choice".to_string())),
            ],
            true,
            Some("beta".to_string()),
        ),
        tools::UserQuestion::new(
            "Second",
            vec![tools::UserOption::new(
                "gamma",
                Some("third choice".to_string()),
            )],
            false,
            None,
        ),
    ]));

    let items = user_interaction_items("runtime-tool-call-7", &suspension);

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "runtime-tool-call-7");
    assert_eq!(items[0].question, "First");
    assert_eq!(items[0].options[0].title, "alpha");
    assert_eq!(items[0].options[0].description, None);
    assert_eq!(items[0].options[1].title, "beta");
    assert_eq!(
        items[0].options[1].description.as_deref(),
        Some("second choice")
    );
    assert!(items[0].multi_select);
    assert_eq!(items[0].default.as_deref(), Some("beta"));

    assert_eq!(items[1].question, "Second");
    assert_eq!(items[1].options[0].title, "gamma");
    assert_eq!(
        items[1].options[0].description.as_deref(),
        Some("third choice")
    );
    assert!(!items[1].multi_select);
    assert_eq!(items[1].default, None);
}
