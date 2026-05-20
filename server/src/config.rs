use serde::Deserialize;
use std::{env, fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub mongo_uri: String,
    pub mongo_database: String,
    pub http_addr: String,
    pub grpc_addr: String,
    pub auth_enabled: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerConfigFile {
    mongo_uri: Option<String>,
    mongo_database: Option<String>,
    http_addr: Option<String>,
    grpc_addr: Option<String>,
    auth_enabled: Option<bool>,
}

impl ServerConfig {
    pub fn load() -> Result<Self, serde_json::Error> {
        let mut config = Self::load_from_paths(
            Some(Path::new(".aemeath/server.json")),
            dirs::home_dir()
                .as_deref()
                .map(|home| home.join(".aemeath/server.json"))
                .as_deref(),
        )?;
        config.apply_env();
        Ok(config)
    }

    pub fn load_from_paths(
        project_path: Option<&Path>,
        global_path: Option<&Path>,
    ) -> Result<Self, serde_json::Error> {
        let mut config = Self::default();
        if let Some(file_config) = read_config_file(global_path)? {
            config.apply_file_config(file_config);
        }
        if let Some(file_config) = read_config_file(project_path)? {
            config.apply_file_config(file_config);
        }
        Ok(config)
    }

    fn apply_file_config(&mut self, file_config: ServerConfigFile) {
        if let Some(mongo_uri) = file_config.mongo_uri {
            self.mongo_uri = mongo_uri;
        }
        if let Some(mongo_database) = file_config.mongo_database {
            self.mongo_database = mongo_database;
        }
        if let Some(http_addr) = file_config.http_addr {
            self.http_addr = http_addr;
        }
        if let Some(grpc_addr) = file_config.grpc_addr {
            self.grpc_addr = grpc_addr;
        }
        if let Some(auth_enabled) = file_config.auth_enabled {
            self.auth_enabled = auth_enabled;
        }
    }

    fn apply_env(&mut self) {
        if let Ok(mongo_uri) = env::var("AEMEATH_SERVER_MONGO_URI") {
            self.mongo_uri = mongo_uri;
        }
        if let Ok(mongo_database) = env::var("AEMEATH_SERVER_DB") {
            self.mongo_database = mongo_database;
        }
        if let Ok(http_addr) = env::var("AEMEATH_SERVER_HTTP_ADDR") {
            self.http_addr = http_addr;
        }
        if let Ok(grpc_addr) = env::var("AEMEATH_SERVER_GRPC_ADDR") {
            self.grpc_addr = grpc_addr;
        }
        if let Ok(auth_enabled) = env::var("AEMEATH_SERVER_AUTH_ENABLED") {
            self.auth_enabled = auth_enabled == "true" || auth_enabled == "1";
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            mongo_uri: "mongodb://localhost:27017/?replicaSet=rs0".to_string(),
            mongo_database: "aemeath".to_string(),
            http_addr: "0.0.0.0:3000".to_string(),
            grpc_addr: "0.0.0.0:50051".to_string(),
            auth_enabled: false,
        }
    }
}

fn read_config_file(path: Option<&Path>) -> Result<Option<ServerConfigFile>, serde_json::Error> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(serde_json::Error::io)?;
    serde_json::from_str(&content).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_server_config_defaults_match_mvp_plan() {
        let config = ServerConfig::load_from_paths(None, None).expect("config loads");

        assert_eq!(
            config.mongo_uri,
            "mongodb://localhost:27017/?replicaSet=rs0"
        );
        assert_eq!(config.mongo_database, "aemeath");
        assert_eq!(config.http_addr, "0.0.0.0:3000");
        assert_eq!(config.grpc_addr, "0.0.0.0:50051");
        assert!(!config.auth_enabled);
    }

    #[test]
    fn test_server_config_project_file_overrides_defaults() {
        let dir = tempdir().expect("tempdir");
        let project_path = dir.path().join("server.json");
        fs::write(
            &project_path,
            r#"{
                "mongoUri": "mongodb://project:27017/?replicaSet=rs0",
                "mongoDatabase": "project_db",
                "httpAddr": "127.0.0.1:3100",
                "grpcAddr": "127.0.0.1:51051",
                "authEnabled": true
            }"#,
        )
        .expect("write project config");

        let config =
            ServerConfig::load_from_paths(Some(&project_path), None).expect("config loads");

        assert_eq!(config.mongo_uri, "mongodb://project:27017/?replicaSet=rs0");
        assert_eq!(config.mongo_database, "project_db");
        assert_eq!(config.http_addr, "127.0.0.1:3100");
        assert_eq!(config.grpc_addr, "127.0.0.1:51051");
        assert!(config.auth_enabled);
    }

    #[test]
    fn test_server_config_project_file_has_priority_over_global_file() {
        let dir = tempdir().expect("tempdir");
        let project_path = dir.path().join("project-server.json");
        let global_path = dir.path().join("global-server.json");
        fs::write(
            &global_path,
            r#"{
                "mongoUri": "mongodb://global:27017/?replicaSet=rs0",
                "mongoDatabase": "global_db",
                "httpAddr": "127.0.0.1:3200",
                "grpcAddr": "127.0.0.1:52051",
                "authEnabled": false
            }"#,
        )
        .expect("write global config");
        fs::write(
            &project_path,
            r#"{
                "mongoUri": "mongodb://project:27017/?replicaSet=rs0",
                "mongoDatabase": "project_db",
                "httpAddr": "127.0.0.1:3100",
                "grpcAddr": "127.0.0.1:51051",
                "authEnabled": true
            }"#,
        )
        .expect("write project config");

        let config = ServerConfig::load_from_paths(Some(&project_path), Some(&global_path))
            .expect("config loads");

        assert_eq!(config.mongo_database, "project_db");
        assert_eq!(config.http_addr, "127.0.0.1:3100");
        assert!(config.auth_enabled);
    }
}
