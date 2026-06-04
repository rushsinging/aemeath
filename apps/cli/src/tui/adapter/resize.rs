//! Retired resize adapter.
//!
//! Resize handling is owned by `App::handle_resize(...)`, which records terminal size, updates
//! output layout metrics, and clears view-state selections directly. Production code must not route
//! resize through an adapter mapping helper.
