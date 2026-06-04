//! Retired input widget adapter.
//!
//! Input state changes are emitted by `InputModel::apply(...)`. Submission extraction lives in
//! `model::input::change::submitted_text_from_changes(...)`, so production code no longer needs an
//! input widget adapter.
