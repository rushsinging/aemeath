//! TUI 面向 SDK 的公共契约。
//!
//! 这些类型只描述 TUI 与 runtime 之间的稳定边界，不依赖具体 TUI
//! 渲染库，也不暴露 runtime 内部的 LLM client、tool registry、task store
//! 或取消 token。

use crate::ChatInputEvent;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub type EventFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
pub type InputEventFuture<'a> = Pin<Box<dyn Future<Output = Vec<ChatInputEvent>> + Send + 'a>>;

/// runtime 返回给 TUI 的 chat handle。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatHandle {
    pub id: crate::ids::ChatId,
}

/// TUI 从 runtime 接收 chat 流式事件的 sink。
pub trait ChatEventSink<Event>: Clone + Send + Sync + 'static {
    fn send_event<'a>(&'a self, event: Event) -> EventFuture<'a>;

    fn try_send_event(&self, event: Event);
}

pub type InputEventOptFuture<'a> =
    Pin<Box<dyn Future<Output = Option<ChatInputEvent>> + Send + 'a>>;

/// runtime drain 忙碌期间追加输入事件的端口。
pub trait ChatInputEventPort: Send + Sync + 'static {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a>;
    /// 阻塞等待下一条输入；None = 通道关闭（shutdown）。
    fn recv_next<'a>(&'a self) -> InputEventOptFuture<'a>;
}

/// TUI 可渲染的图片输入视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImageView {
    pub base64: String,
    pub media_type: String,
    pub final_size: usize,
    pub display_path: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// `ClipboardImageView` → `ChatInputImage`（TUI→Runtime 通道）。
///
/// 默认 `id` 为空字符串（无占位上下文），实际提交时由 `ImageSpan::placeholder()`
/// 覆写。`TUI 内部` `ImageSpan` 仍持有完整 6 字段（`final_size/display_path/
/// width/height` 是 TUI UI 元数据），`ChatInputImage` 是 3 字段窄接口——
/// runtime 不需要 TUI 渲染元数据。
impl From<ClipboardImageView> for crate::ChatInputImage {
    fn from(value: ClipboardImageView) -> Self {
        Self {
            id: String::new(),
            base64: value.base64,
            media_type: value.media_type,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReflectionConfigView {
    pub enabled: bool,
    pub interval_runs: usize,
    pub auto_apply_suggestions: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryConfigView {
    pub enabled: bool,
    pub max_entries: usize,
    pub similarity_threshold: f32,
    pub reflection: ReflectionConfigView,
}

impl Default for MemoryConfigView {
    fn default() -> Self {
        Self {
            enabled: false,
            max_entries: 0,
            similarity_threshold: 0.0,
            reflection: ReflectionConfigView::default(),
        }
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct SkillView {
    pub name: String,
    pub aliases: Vec<String>,
    /// 可选 Slash Command 名；`None` 表示仅保留 Skill identity，不暴露 slash。
    pub slash_command: Option<String>,
    /// `slash_command` 的合法别名，不复用 Skill identity aliases。
    pub slash_aliases: Vec<String>,
    pub description: String,
    pub argument_hint: Option<String>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct SkillSlashRouteView {
    pub skill: String,
    pub slash_command: String,
    pub aliases: Vec<String>,
    pub argument_hint: Option<String>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct SkillsUpdatedEvent {
    pub revision: String,
    pub skills: Vec<SkillView>,
    pub slash_routes: Vec<SkillSlashRouteView>,
}

/// 终端粘贴内容的分类结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteKind {
    /// 空白粘贴：是否读取剪贴板图片由调用方决定。
    Empty,
    /// 本地图片文件路径，已完成 `file://` 与 shell 转义解码（未做文件系统校验）。
    LocalImageFile(PathBuf),
    /// 其余文本，包含 `http(s)`、`data:` 等远端图片链接。
    Text,
}

/// 分类终端粘贴文本。
///
/// 远端链接（`http://`、`https://`、`data:` 等）**MUST** 按文本处理：它们不是本地文件，
/// 按路径读取必然失败，**NEVER** 当作图片导入。
pub fn classify_paste(text: &str) -> PasteKind {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return PasteKind::Empty;
    }
    match resolve_local_image_path(trimmed) {
        Some(path) => PasteKind::LocalImageFile(path),
        None => PasteKind::Text,
    }
}

/// 把终端粘贴的本地图片引用解析为文件路径。
///
/// 终端会把本地图片粘贴成两种形式，两者 **MUST** 都被识别：
/// - `file://` URL（Finder、飞书等应用写入剪贴板的文件引用），含百分号编码与 `localhost` 主机；
/// - shell 转义路径（Ghostty 等终端把空格转义为 `\ `）。
///
/// 带 scheme 的远端链接返回 `None`；本函数不做文件系统校验，存在性由调用方检查。
pub fn resolve_local_image_path(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    let path_text = match split_scheme(trimmed) {
        Some((scheme, after_scheme)) if scheme.eq_ignore_ascii_case("file") => {
            decode_file_url_path(after_scheme)
        }
        Some(_) => return None,
        None => decode_backslash_escaped_path(trimmed),
    };
    has_image_extension(&path_text).then(|| PathBuf::from(path_text))
}

/// 拆分 `scheme:` 前缀；单字母前缀视为 Windows 盘符而非 scheme。
fn split_scheme(text: &str) -> Option<(&str, &str)> {
    let (scheme, after_scheme) = text.split_once(':')?;
    let looks_like_scheme = scheme.len() >= 2
        && scheme
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "+-. ".contains(character));
    looks_like_scheme.then_some((scheme, after_scheme))
}

/// `file:` 之后的部分 → 本地路径：跳过 `//` 与 `localhost` 主机，剥离 `?`/`#` 后缀，还原百分号编码。
fn decode_file_url_path(after_scheme: &str) -> String {
    let without_slashes = after_scheme.trim_start_matches("//");
    let without_host = without_slashes
        .strip_prefix("localhost")
        .unwrap_or(without_slashes);
    let path_part = without_host
        .split(['?', '#'])
        .next()
        .unwrap_or(without_host);
    percent_decode(path_part)
}

/// 还原 `%XX` 百分号编码；非法序列按原样保留。
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if let (Some(high), Some(low)) = (bytes.get(index + 1), bytes.get(index + 2)) {
                if let Some(byte) = hex_byte(*high, *low) {
                    decoded.push(byte);
                    index += 3;
                    continue;
                }
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_byte(high: u8, low: u8) -> Option<u8> {
    let high = (high as char).to_digit(16)?;
    let low = (low as char).to_digit(16)?;
    Some((high * 16 + low) as u8)
}

/// 还原 shell 反斜杠转义（`\ ` → 空格）；非转义字符前的 `\` 原样保留。
fn decode_backslash_escaped_path(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(current) = characters.next() {
        if current != '\\' {
            decoded.push(current);
            continue;
        }
        match characters.clone().next() {
            Some(escaped) if is_shell_escapable(escaped) => {
                decoded.push(escaped);
                characters.next();
            }
            _ => decoded.push('\\'),
        }
    }
    decoded
}

/// shell 中需要反斜杠转义的字符集，终端粘贴路径时按该集合转义。
fn is_shell_escapable(character: char) -> bool {
    matches!(
        character,
        ' ' | '!'
            | '"'
            | '#'
            | '$'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | ';'
            | '<'
            | '>'
            | '?'
            | '['
            | '\\'
            | ']'
            | '^'
            | '`'
            | '{'
            | '|'
            | '}'
            | '~'
    )
}

/// 图片扩展名白名单（大小写不敏感）。
fn has_image_extension(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// SDK 级 TUI 启动上下文。
#[derive(Debug, Clone)]
pub struct TuiLaunchContext {
    pub session_id: String,
    pub cwd: PathBuf,
    pub model_display: String,
    pub memory_config: MemoryConfigView,
    pub skill_snapshot: SkillsUpdatedEvent,
    pub initial_resume_id: Option<String>,
}

/// 会话 reminder 视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderView {
    pub id: String,
    pub content: String,
    pub done: bool,
    pub created_at: u64,
}

impl ReminderView {
    pub fn active(reminders: &[ReminderView]) -> Vec<&ReminderView> {
        reminders.iter().filter(|r| !r.done).collect()
    }

    pub fn recap_line(reminders: &[ReminderView]) -> Option<String> {
        let active: Vec<&str> = reminders
            .iter()
            .filter(|r| !r.done)
            .map(|r| r.content.as_str())
            .collect();
        if active.is_empty() {
            None
        } else {
            Some(format!("* recap: {}", active.join(" | ")))
        }
    }
}

#[cfg(test)]
#[path = "tui_tests.rs"]
mod tests;
