use runtime::api::bootstrap::{ChatBootstrapArgs, ChatModeSelection};

#[test]
fn test_chat_bootstrap_args_selects_tui_by_default() {
    let args = ChatBootstrapArgs {
        tui: true,
        no_tui: false,
        ..Default::default()
    };

    assert_eq!(args.mode_selection(), ChatModeSelection::Tui);
}

#[test]
fn test_chat_bootstrap_args_no_tui_flag_wins() {
    let args = ChatBootstrapArgs {
        tui: true,
        no_tui: true,
        ..Default::default()
    };

    assert_eq!(args.mode_selection(), ChatModeSelection::NoTui);
}

#[test]
fn test_chat_bootstrap_args_disabled_tui_uses_no_tui() {
    let args = ChatBootstrapArgs {
        tui: false,
        no_tui: false,
        ..Default::default()
    };

    assert_eq!(args.mode_selection(), ChatModeSelection::NoTui);
}
