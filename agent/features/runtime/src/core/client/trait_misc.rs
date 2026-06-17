use sdk::{ClipboardImageView, SdkError};

use super::accessors::AgentClientImpl;
use crate::core::port::HookNotificationPort;
use crate::utils::adapter::HookRunnerAdapter;

type Result<T> = std::result::Result<T, SdkError>;

pub(super) async fn notify_hook_impl(
    me: &AgentClientImpl,
    message: &str,
    kind: &str,
) -> Result<()> {
    if let Some(ref runner) = me.inner.hook_runner {
        let adapter = HookRunnerAdapter::new(runner.clone());
        adapter.on_notification(message, kind).await;
    }
    Ok(())
}

pub(super) async fn read_clipboard_image_impl(_me: &AgentClientImpl) -> Result<ClipboardImageView> {
    crate::utils::image::read_clipboard_image()
        .await
        .map(super::mapping::processed_image_to_sdk)
        .map_err(|e| SdkError::Internal(e.to_string()))
}

pub(super) async fn process_image_file_impl(
    _me: &AgentClientImpl,
    path: String,
) -> Result<ClipboardImageView> {
    crate::utils::image::process_image_file(&path)
        .await
        .map(super::mapping::processed_image_to_sdk)
        .map_err(|e| SdkError::Internal(e.to_string()))
}
