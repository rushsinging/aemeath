//! Update feature 内部数据结构（不跨 crate 暴露）。

use serde::Deserialize;

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
