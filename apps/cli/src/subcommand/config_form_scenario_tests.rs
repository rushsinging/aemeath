use ratatui::backend::TestBackend;
use ratatui::Terminal;

use super::*;

fn provider_view() -> sdk::ConfigFormView {
    sdk::ConfigFormView {
        workflow_id: sdk::ConfigFormWorkflowId("provider_connect".to_string()),
        session_id: sdk::ConfigFormSessionId("session-1".to_string()),
        revision: sdk::ConfigFormRevision(0),
        origin: sdk::ConfigFormOrigin::ExplicitCommand,
        page: sdk::ConfigFormPage {
            id: sdk::ConfigFormPageId("select_provider".to_string()),
            title: "选择 Provider".to_string(),
            description: Some("选择一个内置 Provider".to_string()),
            step: Some(sdk::ConfigFormStep {
                current: 1,
                total: 8,
            }),
            fields: vec![sdk::ConfigFormField {
                id: sdk::ConfigFormFieldId("provider_source".to_string()),
                label: "Provider".to_string(),
                description: None,
                field_type: sdk::ConfigFormFieldType::SingleSelect,
                required: true,
                has_value: false,
                display_value: None,
                options: vec![
                    sdk::ConfigFormOption {
                        id: sdk::ConfigFormOptionId("Anthropic".to_string()),
                        label: "Anthropic".to_string(),
                        description: Some("anthropic".to_string()),
                    },
                    sdk::ConfigFormOption {
                        id: sdk::ConfigFormOptionId("OpenAI".to_string()),
                        label: "OpenAI".to_string(),
                        description: Some("openai".to_string()),
                    },
                ],
                error: None,
            }],
            error: None,
            actions: vec![sdk::ConfigFormAction {
                id: sdk::ConfigFormActionId("cancel".to_string()),
                label: "取消".to_string(),
                style: sdk::ConfigFormActionStyle::Destructive,
                shortcut: Some("Esc".to_string()),
            }],
        },
        busy: None,
        terminal: None,
    }
}

fn screen_with_interaction(
    view: &sdk::ConfigFormView,
    width: u16,
    height: u16,
    interaction: ConfigFormInteraction,
) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_config_form(frame, view, "", 0, interaction))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|row| {
            (0..width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn screen(view: &sdk::ConfigFormView, width: u16, height: u16) -> String {
    screen_with_interaction(
        view,
        width,
        height,
        ConfigFormInteraction {
            focused_field: 0,
            selected_option: 0,
            focused_action: 0,
            input_cursor_column: None,
        },
    )
}

#[test]
fn selected_provider_is_visibly_marked_after_navigation() {
    let screen = screen_with_interaction(
        &provider_view(),
        80,
        20,
        ConfigFormInteraction {
            focused_field: 0,
            selected_option: 1,
            focused_action: 0,
            input_cursor_column: None,
        },
    );

    assert!(screen.contains("● OpenAI"));
    assert!(screen.contains("○ Anthropic"));
}

#[test]
fn selected_action_is_visibly_marked() {
    let mut view = provider_view();
    view.page.fields.clear();
    view.page.actions.push(sdk::ConfigFormAction {
        id: sdk::ConfigFormActionId("confirm".to_string()),
        label: "确认".to_string(),
        style: sdk::ConfigFormActionStyle::Primary,
        shortcut: None,
    });
    let screen = screen_with_interaction(
        &view,
        80,
        20,
        ConfigFormInteraction {
            focused_field: 0,
            selected_option: 0,
            focused_action: 1,
            input_cursor_column: None,
        },
    );

    assert!(screen.contains("[确 认 ]"), "{screen}");
    assert!(!screen.contains("[取 消 ]"));
}

#[test]
fn focused_text_input_sets_visible_terminal_cursor() {
    let mut view = provider_view();
    view.page.id = sdk::ConfigFormPageId("edit_endpoint".to_string());
    view.page.fields[0].field_type = sdk::ConfigFormFieldType::Text;
    view.page.fields[0].options.clear();
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            render_config_form(
                frame,
                &view,
                "https://example.test",
                0,
                ConfigFormInteraction {
                    focused_field: 0,
                    selected_option: 0,
                    focused_action: 0,
                    input_cursor_column: Some(8),
                },
            )
        })
        .unwrap();

    assert_eq!(terminal.get_cursor_position().unwrap().x, 11);
    assert!(terminal.get_cursor_position().unwrap().y > 0);
}

#[test]
fn text_input_page_publishes_editing_shortcuts() {
    let mut view = provider_view();
    view.page.id = sdk::ConfigFormPageId("edit_endpoint".to_string());
    view.page.fields[0].field_type = sdk::ConfigFormFieldType::Text;
    view.page.fields[0].options.clear();
    let screen = screen_with_interaction(
        &view,
        80,
        20,
        ConfigFormInteraction {
            focused_field: 0,
            selected_option: 0,
            focused_action: 0,
            input_cursor_column: Some(0),
        },
    );

    assert!(screen.contains("←→ 移 动 光 标"), "{screen}");
    assert!(screen.contains("Esc 返 回"), "{screen}");
}

#[test]
fn wide_screen_renders_two_columns_without_row_drift() {
    let screen = screen(&provider_view(), 100, 24);
    assert!(screen.contains("Anthropic"));
    assert!(screen.contains("1/8"));
    assert!(screen.lines().all(|line| line.chars().count() == 100));
}

#[test]
fn narrow_screen_falls_back_to_single_column() {
    let screen = screen(&provider_view(), 44, 24);
    assert!(screen.contains("Anthropic"));
    assert!(screen.lines().all(|line| line.chars().count() == 44));
}

#[test]
fn replacing_page_clears_previous_frame_content() {
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            render_config_form(
                frame,
                &provider_view(),
                "",
                0,
                ConfigFormInteraction {
                    focused_field: 0,
                    selected_option: 0,
                    focused_action: 0,
                    input_cursor_column: None,
                },
            )
        })
        .unwrap();
    let mut next = provider_view();
    next.page.title = "设置 Base URL".to_string();
    next.page.fields.clear();
    terminal
        .draw(|frame| {
            render_config_form(
                frame,
                &next,
                "https://example.test",
                0,
                ConfigFormInteraction {
                    focused_field: 0,
                    selected_option: 0,
                    focused_action: 0,
                    input_cursor_column: None,
                },
            )
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let screen = (0..20)
        .map(|row| {
            (0..80)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(screen.contains("Base URL"));
    assert!(!screen.contains("Anthropic"));
}

#[test]
fn secret_input_never_reaches_screen() {
    let mut view = provider_view();
    view.page.fields[0].field_type = sdk::ConfigFormFieldType::Secret;
    view.page.fields[0].options.clear();
    let screen = screen(&view, 80, 20);
    assert!(!screen.contains("secret-key"));
}
