mod context;
mod file_sink;
mod formatter;
mod lifecycle;

pub use context::{
    app_version, boot_ts, capture, current_chat_id, current_model, current_provider,
    current_request_id, current_role, current_turn, instrument, session_id, set_app_version,
    set_boot_ts, set_current_chat_id, set_current_model, set_current_provider,
    set_current_request_id, set_current_role, set_current_turn, set_session_id, spawn_instrumented,
    within,
};
pub use file_sink::UnifiedLogger;
pub use formatter::timestamp_local_rfc3339;
pub use lifecycle::{is_rotated_log_path, rotated_path, timestamp_rfc3339};
