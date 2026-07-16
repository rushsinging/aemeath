use std::path::PathBuf;

use crate::tui::app::App;

pub fn app() -> App {
    App::new(
        "scenario-session".to_owned(),
        PathBuf::from("/workspace"),
        "scenario-model".to_owned(),
    )
}
