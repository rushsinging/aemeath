use crate::domain::types::read::{ReadInput, ReadResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct FileReadTool;

#[async_trait]
impl TypedTool for FileReadTool {
    type Output = ReadResult;
    fn name(&self) -> &str {
        "Read"
    }
    fn description(&self) -> &str {
        "Reads a file from the local filesystem. Supports text files (with line numbers) and images (PNG, JPG, GIF, WebP). Cannot read directories."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::filesystem::file_read(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        ReadInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        ReadResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<ReadResult> {
        let args: ReadInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let requested_path = args.file_path.as_str();
        let path = match ctx.workspace_read().resolve_file_path_authorized(
            Path::new(requested_path),
            ctx.authorization().allow_outside_workspace,
        ) {
            Ok(path) => path,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let file_path = path.to_string_lossy().into_owned();
        if !path.exists() {
            return TypedToolResult::error(format!("file not found: {file_path}"));
        }

        // Check if the file is an image
        if is_image_extension(&file_path) {
            return read_image_file(&file_path, &path).await;
        }

        let offset = args.offset.unwrap_or(0) as usize;
        let limit = args.limit.unwrap_or(2000) as usize;
        match read_text_window(&path, offset, limit).await {
            Ok(window) => {
                // Track this file as read
                ctx.read_set().record(&file_path);
                ctx.read_set().record(path.to_string_lossy().as_ref());
                if window.lines.is_empty() {
                    let data = ReadResult {
                        content: String::new(),
                        file_path: file_path.to_string(),
                        line_count: 0,
                        start_line: 0,
                        total_lines: window.reached_eof.then_some(window.consumed_lines as u64),
                    };
                    TypedToolResult::success("(empty file)", data)
                } else {
                    let end_line = window.start_line.saturating_add(window.lines.len());
                    let num_width = end_line.to_string().len();
                    let numbered = window
                        .lines
                        .iter()
                        .enumerate()
                        .map(|(line_index, line)| {
                            format!(
                                "{:>width$}  {}",
                                window.start_line + line_index + 1,
                                line,
                                width = num_width
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let data = ReadResult {
                        content: numbered.clone(),
                        file_path: file_path.to_string(),
                        line_count: window.lines.len() as u64,
                        start_line: window.start_line as u64,
                        total_lines: window.reached_eof.then_some(window.consumed_lines as u64),
                    };
                    // output = 完整带行号内容（给 LLM，经 to_llm_view text-first）；
                    // data = 同样内容的结构化 ReadResult（给 TUI）。
                    TypedToolResult::success(numbered, data)
                }
            }
            Err(error) => TypedToolResult::error(format!("failed to read file: {error}")),
        }
    }
}

struct TextWindow {
    lines: Vec<String>,
    start_line: usize,
    consumed_lines: usize,
    reached_eof: bool,
}

async fn read_text_window(path: &Path, offset: usize, limit: usize) -> std::io::Result<TextWindow> {
    let file = tokio::fs::File::open(path).await?;
    let mut lines = BufReader::new(file).lines();
    let mut consumed_lines = 0usize;
    while consumed_lines < offset {
        if lines.next_line().await?.is_none() {
            return Ok(TextWindow {
                lines: Vec::new(),
                start_line: 0,
                consumed_lines,
                reached_eof: true,
            });
        }
        consumed_lines = consumed_lines.saturating_add(1);
    }

    let start_line = consumed_lines;
    let mut window_lines = Vec::with_capacity(limit.min(2_000));
    let mut reached_eof = false;
    while window_lines.len() < limit {
        let Some(line) = lines.next_line().await? else {
            reached_eof = true;
            break;
        };
        consumed_lines = consumed_lines.saturating_add(1);
        window_lines.push(line);
    }
    Ok(TextWindow {
        lines: window_lines,
        start_line,
        consumed_lines,
        reached_eof,
    })
}

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

fn is_image_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

async fn read_image_file(file_path: &str, path: &Path) -> TypedToolResult<ReadResult> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let data = match tokio::fs::read(path).await {
        Ok(d) => d,
        Err(e) => return TypedToolResult::error(format!("failed to read image: {e}")),
    };

    if data.is_empty() {
        return TypedToolResult::error("image file is empty");
    }

    let media_type = detect_media_type(&data, file_path);
    let size = data.len();
    let base64 = STANDARD.encode(&data);

    // 5MB base64 limit
    if base64.len() > 5 * 1024 * 1024 {
        return TypedToolResult::error(format!(
            "image too large: {} bytes (base64: {} bytes, max: 5MB)",
            size,
            base64.len()
        ));
    }

    TypedToolResult::success(
        format!("Image: {}", file_path),
        ReadResult {
            content: format!("Image: {} ({} bytes, {})", file_path, size, media_type),
            file_path: file_path.to_string(),
            line_count: 0,
            start_line: 0,
            total_lines: None,
        },
    )
    .with_image(base64, media_type)
}

fn detect_media_type(data: &[u8], path: &str) -> String {
    if data.len() >= 4 {
        if data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4e && data[3] == 0x47 {
            return "image/png".to_string();
        }
        if data[0] == 0xff && data[1] == 0xd8 && data[2] == 0xff {
            return "image/jpeg".to_string();
        }
        if data[0] == 0x47 && data[1] == 0x49 && data[2] == 0x46 {
            return "image/gif".to_string();
        }
        if data.len() >= 12 && data[0] == 0x52 && data[8] == 0x57 && data[9] == 0x45 {
            return "image/webp".to_string();
        }
    }
    // Fallback to extension
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
    .to_string()
}

#[cfg(test)]
#[path = "file_read_tests.rs"]
mod tests;
