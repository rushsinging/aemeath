pub mod completion;
pub mod core;
pub mod display;
pub mod input;
pub mod output_area;
pub mod session;

pub use self::core::App;
pub use self::display::status_bar::StatusBar;
pub use self::input::input_area::InputArea;
pub use self::output_area::OutputArea;

pub(crate) fn messages_to_sdk(
    messages: &[::runtime::api::core::message::Message],
) -> Vec<sdk::ChatMessage> {
    messages
        .iter()
        .map(|message| sdk::ChatMessage {
            role: match message.role {
                ::runtime::api::core::message::Role::User => "user".to_string(),
                ::runtime::api::core::message::Role::Assistant => "assistant".to_string(),
            },
            content: serde_json::to_value(&message.content).unwrap_or(serde_json::Value::Null),
        })
        .collect()
}
