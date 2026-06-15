use sdk::SdkError;

use super::accessors::AgentClientImpl;

type Result<T> = std::result::Result<T, SdkError>;

pub(super) async fn list_reminders_impl(me: &AgentClientImpl) -> Result<Vec<sdk::ReminderView>> {
    let reminders = me.inner.session_reminders.read().unwrap();
    Ok(reminders
        .list()
        .iter()
        .map(|r| sdk::ReminderView {
            id: r.id.clone(),
            content: r.content.clone(),
            done: r.done,
            created_at: r.created_at,
        })
        .collect())
}

pub(super) async fn add_reminder_impl(me: &AgentClientImpl, content: &str) -> Result<String> {
    let id = uuid::Uuid::now_v7().to_string();
    let created_at = current_timestamp_secs();
    me.inner
        .session_reminders
        .write()
        .unwrap()
        .add(id, content, created_at)
        .map_err(|e| SdkError::Internal(format!("添加 reminder 失败: {e}")))
}

pub(super) async fn complete_reminder_impl(me: &AgentClientImpl, id: &str) -> Result<()> {
    me.inner
        .session_reminders
        .write()
        .unwrap()
        .complete(id)
        .map_err(|e| SdkError::Internal(format!("完成 reminder 失败: {e}")))
}

fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
