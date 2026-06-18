//! Update feature 内部数据结构（不跨 crate 暴露）。

use serde::{Deserialize, Serialize};

/// 缓存文件结构（`~/.agents/update_check.json`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CacheEntry {
    /// 上次检查时间（ISO 8601 / RFC 3339）。
    pub last_check: String,
    /// 最新版本号（不含 `v` 前缀）。
    pub latest_version: String,
    /// Release 页面 URL。
    pub latest_url: String,
}

/// GitHub Releases API 响应（仅提取需要的字段）。
#[derive(Debug, Deserialize)]
pub(crate) struct GitHubRelease {
    /// Tag 名称，如 `v0.9.0`。
    pub tag_name: String,
    /// Release HTML URL。
    pub html_url: String,
    /// Release notes body。
    #[serde(default)]
    pub body: Option<String>,
    /// 是否为 pre-release。
    #[serde(default)]
    pub prerelease: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_entry_serialize_deserialize() {
        let entry = CacheEntry {
            last_check: "2026-06-18T12:00:00Z".to_string(),
            latest_version: "0.9.0".to_string(),
            latest_url: "https://github.com/rushsinging/aemeath/releases/tag/v0.9.0".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: CacheEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.latest_version, "0.9.0");
    }

    #[test]
    fn test_github_release_deserialize() {
        let json = r#"{
            "tag_name": "v0.9.0",
            "html_url": "https://github.com/rushsinging/aemeath/releases/tag/v0.9.0",
            "body": "Release notes here",
            "prerelease": false
        }"#;
        let release: GitHubRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v0.9.0");
        assert_eq!(
            release.html_url,
            "https://github.com/rushsinging/aemeath/releases/tag/v0.9.0"
        );
        assert_eq!(release.body.as_deref(), Some("Release notes here"));
        assert!(!release.prerelease);
    }

    #[test]
    fn test_github_release_deserialize_missing_optional() {
        let json = r#"{
            "tag_name": "v1.0.0-beta",
            "html_url": "https://example.com"
        }"#;
        let release: GitHubRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v1.0.0-beta");
        assert!(release.body.is_none());
        assert!(!release.prerelease);
    }
}
