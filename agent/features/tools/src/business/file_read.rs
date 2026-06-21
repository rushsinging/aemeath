use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::read::{ReadInput, ReadResult};
use share::tool::{PathAccess, PathKind};
use std::path::Path;

pub struct FileReadTool;

const FILE_ACCESS: [PathAccess; 1] = [PathAccess {
    field: "file_path",
    kind: PathKind::File,
}];

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
        use share::tool::types::ToolSchema;
        ReadInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        ReadResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn path_accesses(&self) -> &'static [PathAccess] {
        &FILE_ACCESS
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<ReadResult> {
        let args: ReadInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let file_path = args.file_path.as_str();

        // Path has already been validated and normalised by PolicyEngine
        let path = std::path::PathBuf::from(file_path);
        if !path.exists() {
            return TypedToolResult::error(format!("file not found: {file_path}"));
        }

        // Check if the file is an image
        if is_image_extension(file_path) {
            return read_image_file(file_path, &path).await;
        }

        let offset = args.offset.unwrap_or(0) as usize;
        let limit = args.limit.unwrap_or(2000) as usize;
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total = lines.len();
                let start = offset.min(total);
                let end = (start + limit).min(total);
                // Use fixed-width line numbers (no tab) to avoid TUI rendering issues
                let num_width = format!("{}", end).len();
                let numbered: String = lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(i, line)| {
                        format!("{:>width$}  {}", start + i + 1, line, width = num_width)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                // Track this file as read
                if let Ok(mut read_files) = ctx.read_files.lock() {
                    read_files.insert(file_path.to_string());
                    read_files.insert(path.to_string_lossy().to_string());
                }
                if numbered.is_empty() {
                    let data = ReadResult {
                        content: String::new(),
                        file_path: file_path.to_string(),
                        line_count: 0,
                        start_line: 0,
                        total_lines: 0,
                    };
                    TypedToolResult::success("(empty file)", data)
                } else {
                    let line_count = end - start;
                    let data = ReadResult {
                        content: numbered.clone(),
                        file_path: file_path.to_string(),
                        line_count: line_count as u64,
                        start_line: start as u64,
                        total_lines: total as u64,
                    };
                    // output = 完整带行号内容（给 LLM，经 to_llm_view text-first）；
                    // data = 同样内容的结构化 ReadResult（给 TUI）。
                    TypedToolResult::success(numbered, data)
                }
            }
            Err(e) => TypedToolResult::error(format!("failed to read file: {e}")),
        }
    }
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
            total_lines: 0,
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
