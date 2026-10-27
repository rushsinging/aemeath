//! plugins 目录 registry（c8）：`plugins/<name>/plugin.md` 声明式规则
//! 落盘 + 解析 + 校验（#2030）。plugin 是数据 NEVER 代码。

use crate::adapters::filesystem::parse_markdown_with_frontmatter;
use crate::application::ComponentRegistry;
use crate::constants::{ERROR_DOMAIN, PLUGIN_MAX_BYTES};
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::errors::HarnessError;
use crate::domain::plugin::{validate_plugin_rule, PluginRule};
use std::path::{Path, PathBuf};

/// 解析 plugin.md 正文为规则集（`## seam: <kind>` 分节 + 节内键值）。
pub fn parse_plugin_rules(body: &str) -> Result<Vec<PluginRule>, HarnessError> {
    let mut rules = Vec::new();
    let mut current_kind: Option<String> = None;
    let mut current_lines: Vec<&str> = Vec::new();

    let flush = |kind: &Option<String>, lines: &[&str], rules: &mut Vec<PluginRule>| {
        let Some(kind) = kind else { return Ok(()) };
        let yaml_body = lines.join("\n");
        let yaml_body = yaml_body.trim();
        if yaml_body.is_empty() {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("seam 节 `{kind}` 缺少参数体"),
            ));
        }
        let rule = match kind.as_str() {
            "trim_output" => {
                let parsed: TrimOutputBody = serde_yml::from_str(yaml_body).map_err(|error| {
                    HarnessError::invalid(
                        ERROR_DOMAIN,
                        format!("trim_output 参数解析失败：{error}"),
                    )
                })?;
                PluginRule::TrimOutput {
                    tool: parsed.tool,
                    max_chars: parsed.max_chars,
                }
            }
            "error_hint" => {
                let parsed: ErrorHintBody = serde_yml::from_str(yaml_body).map_err(|error| {
                    HarnessError::invalid(ERROR_DOMAIN, format!("error_hint 参数解析失败：{error}"))
                })?;
                PluginRule::ErrorHint {
                    tool: parsed.tool,
                    pattern: parsed.pattern,
                    hint: parsed.hint,
                }
            }
            other => {
                return Err(HarnessError::invalid(
                    ERROR_DOMAIN,
                    format!("未知 seam 类型：{other}（支持 trim_output / error_hint；更多 seam 随 #2069 重设计）"),
                ));
            }
        };
        validate_plugin_rule(&rule)?;
        rules.push(rule);
        Ok(())
    };

    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(kind) = trimmed.strip_prefix("## seam: ") {
            flush(&current_kind, &current_lines, &mut rules)?;
            current_kind = Some(kind.trim().to_string());
            current_lines.clear();
        } else if current_kind.is_some() {
            current_lines.push(line);
        }
    }
    flush(&current_kind, &current_lines, &mut rules)?;
    Ok(rules)
}

#[derive(serde::Deserialize)]
struct TrimOutputBody {
    #[serde(default)]
    tool: String,
    max_chars: usize,
}

#[derive(serde::Deserialize)]
struct ErrorHintBody {
    #[serde(default)]
    tool: String,
    pattern: String,
    hint: String,
}

/// plugins 目录组件注册表（每 run 扫描，天然热载）。
pub struct PluginsDirectoryRegistry {
    directory: PathBuf,
}

impl PluginsDirectoryRegistry {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn plugin_path(&self, name: &ComponentName) -> PathBuf {
        self.directory.join(name.as_str()).join("plugin.md")
    }

    fn plugin_id(name: &str) -> ComponentId {
        ComponentId {
            kind: ComponentKind::Plugin,
            name: ComponentName::new(name),
        }
    }

    /// 原子写（tmp+rename）。
    fn atomic_write(path: &Path, text: &str) -> Result<(), HarnessError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("创建目录失败：{error}"))
            })?;
        }
        let temp_path = path.with_extension("md.tmp");
        std::fs::write(&temp_path, text).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("写临时文件失败：{error}"))
        })?;
        std::fs::rename(&temp_path, path).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("原子替换失败：{error}"))
        })?;
        Ok(())
    }
}

impl ComponentRegistry for PluginsDirectoryRegistry {
    fn handles(&self, id: &ComponentId) -> bool {
        id.kind == ComponentKind::Plugin
    }

    fn list(&self) -> Vec<ComponentId> {
        let Ok(entries) = std::fs::read_dir(&self.directory) else {
            return Vec::new();
        };
        let mut ids: Vec<ComponentId> = entries
            .flatten()
            .filter(|entry| entry.path().join("plugin.md").is_file())
            .filter_map(|entry| entry.file_name().to_str().map(Self::plugin_id))
            .collect();
        ids.sort();
        ids
    }

    fn read(&self, id: &ComponentId) -> Result<ComponentContent, HarnessError> {
        if !self.handles(id) {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("PluginsDirectoryRegistry 只服务 Plugin 组件：{id}"),
            ));
        }
        let path = self.plugin_path(&id.name);
        let source = std::fs::read_to_string(&path).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("读取 plugin.md 失败 {}: {error}", path.display()),
            )
        })?;
        parse_markdown_with_frontmatter(&source)
    }

    fn validate(&self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        if !self.handles(id) {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("PluginsDirectoryRegistry 只服务 Plugin 组件：{id}"),
            ));
        }
        if content.body.len() as u64 > PLUGIN_MAX_BYTES {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("plugin.md 超过大小上限 {PLUGIN_MAX_BYTES} 字节"),
            ));
        }
        // 以落盘文本（body）解析为准：frontmatter 字段与 body 可能不同步，
        // write 落盘的是 body（契约与 skills registry 对齐）。
        let parsed = parse_markdown_with_frontmatter(&content.body)?;
        let description = parsed
            .frontmatter
            .as_ref()
            .and_then(|frontmatter| frontmatter.get("description"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if description.trim().is_empty() {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                "plugin.md frontmatter 缺少非空 description",
            ));
        }
        parse_plugin_rules(&content.body)?;
        Ok(())
    }

    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        self.validate(id, content)?;
        let path = self.plugin_path(&id.name);
        Self::atomic_write(&path, &content.body)
    }

    fn delete(&mut self, id: &ComponentId) -> Result<(), HarnessError> {
        let plugin_dir = self.directory.join(id.name.as_str());
        match std::fs::remove_dir_all(&plugin_dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("删除 plugin 目录失败 {}: {error}", plugin_dir.display()),
            )),
        }
    }
}
