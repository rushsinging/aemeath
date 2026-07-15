//! Shared provider error helpers.

pub(crate) fn is_user_cancelled_provider_error(error: &provider::api::LlmError) -> bool {
    error.is_cancelled()
}
