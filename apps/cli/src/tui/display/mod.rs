pub mod dialog;
mod render;
pub mod safe_text;
pub mod status_bar;
pub(crate) mod stream;
pub mod syntax;
pub mod task_list;
pub mod task_window;
pub mod theme;

#[cfg(test)]
mod status_path_tests;
#[cfg(test)]
pub(crate) mod task_window_helpers_tests;
#[cfg(test)]
mod task_window_progress_tests;
#[cfg(test)]
mod task_window_tests;

pub use status_bar::{StatusBar, StatusBarRow, StatusType, WorktreeKind};
pub use theme::*;
