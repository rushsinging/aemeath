use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Normalize and validate a file path against the workspace boundary.
/// Returns the normalized path if valid, or an error message if it escapes the workspace.
fn validate_and_normalize_path(file_path: &str, workspace_root: &Path) -> Result<PathBuf, String> {
    // Convert to absolute path
    let abs_path = if Path::new(file_path).is_absolute() {
        PathBuf::from(file_path)
    } else {
        workspace_root.join(file_path)
    };
    
    // Normalize the path (resolve .., symlinks, etc.)
    let normalized = abs_path.canonicalize()
        .or_else(|_| {
            // If canonicalize fails (e.g., file doesn't exist yet), use parent resolution
            let parent = abs_path.parent().unwrap_or(&abs_path);
            parent.canonicalize().map(|p| p.join(abs_path.file_name().unwrap_or_default()))
        })
        .unwrap_or_else(|_| abs_path);
    
    // Convert workspace root to absolute and normalize
    let workspace_abs = workspace_root.canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    
    // Check that the normalized path is within the workspace
    let normalized_str = normalized.to_string_lossy();
    let workspace_str = workspace_abs.to_string_lossy();
    
    if !normalized_str.starts_with(&*workspace_str) {
        return Err(format!(
            "Path '{}' escapes workspace '{}'. Only files within the workspace are allowed.",
            normalized_str, workspace_str
        ));
    }
    
    Ok(normalized)
}

/// Check if a path attempts directory traversal
fn has_traversal_attempt(file_path: &str) -> bool {
    file_path.contains("..")
}

pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str { "Read" }
    fn description(&self) -> &str {
        "Reads a file from the local filesystem.\n\nUsage:\n- The file_path parameter must be an absolute path, not a relative path\n- By default, it reads up to 2000 lines starting from the beginning of the file\n- When you already know which part of the file you need, only read that part. This can be important for larger files.\n- Results are returned using cat -n format, with line numbers starting at 1\n- This tool allows reading images (PNG, JPG, GIF, WebP). When reading an image file the contents are presented visually.\n- This tool can only read files, not directories. To read a directory, use an ls command via the Bash tool.\n- If you read a file that exists but has empty contents you will receive a warning."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Absolute path to the file to read" },
                "offset": { "type": "integer", "description": "Line number to start reading from (0-based)" },
                "limit": { "type": "integer", "description": "Maximum number of lines to read (default 2000)" }
            },
            "required": ["file_path"]
        })
    }
    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let file_path = match input.get("file_path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("missing required parameter: file_path"),
        };
          
        // Check for directory traversal attempts
        if has_traversal_attempt(file_path) {
            return ToolResult::error(format!(
                "Directory traversal attempt detected: {}\nOnly files within the workspace are allowed.",
                file_path
            ));
        }
          
        // Validate path is within workspace boundary
        let path = match validate_and_normalize_path(file_path, &ctx.cwd) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(e),
        };
          
        if !path.exists() { return ToolResult::error(format!("file not found: {file_path}")); }

        // Check if the file is an image
        if is_image_extension(file_path) {
            return read_image_file(file_path, &path).await;
        }

        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total = lines.len();
                let start = offset.min(total);
                let end = (start + limit).min(total);
                // Use fixed-width line numbers (no tab) to avoid TUI rendering issues
                let num_width = format!("{}", end).len();
                let numbered: String = lines[start..end].iter().enumerate()
                    .map(|(i, line)| format!("{:>width$}  {}", start + i + 1, line, width = num_width))
                    .collect::<Vec<_>>().join("\n");
                // Track this file as read
                if let Ok(mut read_files) = ctx.read_files.lock() {
                    read_files.insert(file_path.to_string());
                }
                if numbered.is_empty() { ToolResult::success("(empty file)") }
                else { ToolResult::success(numbered) }
            }
            Err(e) => ToolResult::error(format!("failed to read file: {e}")),
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

async fn read_image_file(file_path: &str, path: &Path) -> ToolResult {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let data = match tokio::fs::read(path).await {
        Ok(d) => d,
        Err(e) => return ToolResult::error(format!("failed to read image: {e}")),
    };

    if data.is_empty() {
        return ToolResult::error("image file is empty");
    }

    let media_type = detect_media_type(&data, file_path);
    let size = data.len();
    let base64 = STANDARD.encode(&data);

    // 5MB base64 limit
    if base64.len() > 5 * 1024 * 1024 {
        return ToolResult::error(format!(
            "image too large: {} bytes (base64: {} bytes, max: 5MB)",
            size, base64.len()
        ));
    }

    let description = format!("Image: {} ({} bytes, {})", file_path, size, media_type);
    ToolResult::success(&description)
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
    }.to_string()
}
