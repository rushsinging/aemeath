use crate::tui::app::App;
use crate::tui::effect::effect::Effect;

fn make_app() -> App {
    App::new(
        "s".to_string(),
        std::path::PathBuf::from("/tmp"),
        "m".to_string(),
    )
}

#[test]
fn reflect_queries_default_limit_without_processing_or_spinner() {
    let mut app = make_app();
    let effects = app.handle_reflect_command("");

    assert_eq!(effects, vec![Effect::QueryReflectionHistory { limit: 10 }]);
    assert!(!app.chat.is_processing);
    assert!(app
        .model
        .conversation
        .activity_observations()
        .activities()
        .is_empty());
}

#[test]
fn reflect_accepts_positive_limit_and_rejects_invalid_limit() {
    let mut app = make_app();
    assert_eq!(
        app.handle_reflect_command("3"),
        vec![Effect::QueryReflectionHistory { limit: 3 }]
    );
    assert!(app.handle_reflect_command("0").is_empty());
    assert!(app.handle_reflect_command("nope").is_empty());
}
