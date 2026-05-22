/// 加载配置文件（供子命令复用）
pub(crate) fn load_config() -> Option<aemeath_core::config::Config> {
    let paths = [
        dirs::home_dir()
            .map(|h| h.join(".aemeath").join("config.json"))
            .unwrap_or_default(),
        std::path::PathBuf::from(".aemeath/config.json"),
    ];
    for path in &paths {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(c) = serde_json::from_str::<aemeath_core::config::Config>(&content) {
                    return Some(c);
                }
            }
        }
    }
    None
}
