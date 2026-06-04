//! Retired output scroll widget adapter.
//!
//! Output scroll truth lives in `OutputViewState`. App synchronizes document metrics with
//! `OutputViewState::sync_document_metrics(...)` from the current layout/live-status projection
//! before rendering. This module intentionally contains no production widget writeback helpers.
