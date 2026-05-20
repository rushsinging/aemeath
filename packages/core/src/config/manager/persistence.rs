//! Config persistence (save / update)

use super::*;

impl ConfigManager {
    /// Save configuration to global file
    pub async fn save_global(&self) -> Result<(), String> {
        let config = self.config.read().await.clone();

        // Ensure parent directory exists
        if let Some(parent) = self.global_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;

        tokio::fs::write(&self.global_path, content)
            .await
            .map_err(|e| format!("Failed to write config: {e}"))?;

        Ok(())
    }

    /// Save configuration to project file
    pub async fn save_project(&self) -> Result<(), String> {
        let project_path = self
            .project_path
            .as_ref()
            .ok_or("No project directory set")?;

        let config = self.config.read().await.clone();

        // Ensure parent directory exists
        if let Some(parent) = project_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;

        tokio::fs::write(project_path, content)
            .await
            .map_err(|e| format!("Failed to write config: {e}"))?;

        Ok(())
    }

    /// Update configuration
    pub async fn update<F>(&self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut Config),
    {
        let mut config = self.config.write().await;
        f(&mut config);
        drop(config);
        self.save_global().await
    }
}
