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
