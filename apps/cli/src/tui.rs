pub mod adapter;
pub mod app;
pub mod effect;
pub mod model;
pub mod render;
pub mod update;
pub mod view_assembler;
pub mod view_model;
pub mod view_state;

#[cfg(test)]
mod log_scope_tests;

pub(crate) const LOG_TARGET: &str = "cli::tui";

#[macro_export]
macro_rules! tui_log_debug {
    ($($arg:tt)*) => {
        log::debug!(target: crate::tui::LOG_TARGET, $($arg)*)
    };
}

#[macro_export]
macro_rules! tui_log_info {
    ($($arg:tt)*) => {
        log::info!(target: crate::tui::LOG_TARGET, $($arg)*)
    };
}

#[macro_export]
macro_rules! tui_log_warn {
    ($($arg:tt)*) => {
        log::warn!(target: crate::tui::LOG_TARGET, $($arg)*)
    };
}

#[macro_export]
macro_rules! tui_log_error {
    ($($arg:tt)*) => {
        log::error!(target: crate::tui::LOG_TARGET, $($arg)*)
    };
}

#[macro_export]
macro_rules! tui_log_trace {
    ($($arg:tt)*) => {
        log::trace!(target: crate::tui::LOG_TARGET, $($arg)*)
    };
}

pub(crate) use {tui_log_info as log_info, tui_log_warn as log_warn};

#[cfg(test)]
mod tests {
    #[test]
    fn test_log_target_uses_cli_prefix() {
        assert_eq!(super::LOG_TARGET, "cli::tui");
    }
}

pub use self::app::App;
pub use self::render::input::input_area::InputArea;
pub use self::render::output_area::OutputArea;
pub use self::render::status::StatusBar;
