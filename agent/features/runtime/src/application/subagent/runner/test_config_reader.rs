use share::config::domain::snapshot::ConfigSnapshot;
use tokio::sync::watch;

pub struct FixedConfigReader {
    snapshot: ConfigSnapshot,
}

impl FixedConfigReader {
    pub fn from_snapshot(snapshot: ConfigSnapshot) -> Self {
        Self { snapshot }
    }
}

#[async_trait::async_trait]
impl config::ConfigReader for FixedConfigReader {
    fn committed_snapshot(&self) -> ConfigSnapshot {
        self.snapshot.clone()
    }

    fn subscribe_committed(&self) -> watch::Receiver<ConfigSnapshot> {
        let (_sender, receiver) = watch::channel(self.snapshot.clone());
        receiver
    }

    async fn refresh_if_sources_changed(&self) -> config::ConfigRefreshOutcome {
        config::ConfigRefreshOutcome::Unchanged
    }
}
