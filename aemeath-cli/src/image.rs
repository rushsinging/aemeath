use std::path::Path;

/// Maximum base64-encoded image size (API enforced)
pub const API_IMAGE_MAX_BASE64_SIZE: usize = 5 * 1024 * 1024; // 5 MB

/// Target raw image size to stay under base64 limit after encoding
pub const IMAGE_TARGET_RAW_SIZE: usize = (API_IMAGE_MAX_BASE64_SIZE * 3) / 4; // 3.75 MB

/// Client-side maximum dimensions for image resizing
pub const IMAGE_MAX_WIDTH: u32 = 2000;

/// Supported image formats
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageFormat {
    pub fn media_type(&self) -> String {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Webp => "image/webp",
        }
        .to_string()
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "png" => Some(ImageFormat::Png),
            "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
            "gif" => Some(ImageFormat::Gif),
            "webp" => Some(ImageFormat::Webp),
            _ => None,
        }
    }
}

/// Detect image format from magic bytes
pub fn detect_image_format(data: &[u8]) -> ImageFormat {
    if data.len() < 4 {
        return ImageFormat::Png; // default
    }

    // PNG: 89 50 4E 47
    if data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4e && data[3] == 0x47 {
        return ImageFormat::Png;
    }

    // JPEG: FF D8 FF
    if data[0] == 0xff && data[1] == 0xd8 && data[2] == 0xff {
        return ImageFormat::Jpeg;
    }

    // GIF: 47 49 46
    if data[0] == 0x47 && data[1] == 0x49 && data[2] == 0x46 {
        return ImageFormat::Gif;
    }

    // WebP: RIFF....WEBP
    if data[0] == 0x52 && data[1] == 0x49 && data[2] == 0x46 && data[3] == 0x46 {
        if data.len() >= 12
            && data[8] == 0x57
            && data[9] == 0x45
            && data[10] == 0x42
            && data[11] == 0x50
        {
            return ImageFormat::Webp;
        }
    }

    ImageFormat::Png // default
}

/// Check if a file path is an image based on extension
pub fn is_image_file(path: &str) -> bool {
    let path = Path::new(path);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    ImageFormat::from_extension(ext).is_some()
}

/// Result of image processing
#[derive(Debug, Clone)]
pub struct ProcessedImage {
    pub base64: String,
    pub media_type: String,
    pub original_size: usize,
    pub final_size: usize,
}

/// Process an image file: read, optionally resize, and encode as base64
pub async fn process_image_file(path: &str) -> Result<ProcessedImage, ImageError> {
    let file_path = Path::new(path);
    if !file_path.exists() {
        return Err(ImageError::FileNotFound(path.to_string()));
    }

    let data = tokio::fs::read(file_path)
        .await
        .map_err(|e| ImageError::ReadError(e.to_string()))?;

    if data.is_empty() {
        return Err(ImageError::EmptyFile);
    }

    let original_size = data.len();
    let format = detect_image_format(&data);
    let media_type = format.media_type();

    // Check if image needs resizing
    let final_data = if original_size > IMAGE_TARGET_RAW_SIZE {
        resize_image(&data, format)?
    } else {
        data
    };

    let base64 = base64_encode(&final_data);
    let final_size = final_data.len();

    // Validate base64 size
    if base64.len() > API_IMAGE_MAX_BASE64_SIZE {
        return Err(ImageError::TooLarge {
            original_size,
            base64_size: base64.len(),
        });
    }

    Ok(ProcessedImage {
        base64,
        media_type,
        original_size,
        final_size,
    })
}

/// Process image data directly (for clipboard images)
pub fn process_image_data(data: &[u8]) -> Result<ProcessedImage, ImageError> {
    if data.is_empty() {
        return Err(ImageError::EmptyFile);
    }

    let original_size = data.len();
    let format = detect_image_format(data);
    let media_type = format.media_type();

    let final_data = if original_size > IMAGE_TARGET_RAW_SIZE {
        resize_image(data, format)?
    } else {
        data.to_vec()
    };

    let base64 = base64_encode(&final_data);
    let final_size = final_data.len();

    if base64.len() > API_IMAGE_MAX_BASE64_SIZE {
        return Err(ImageError::TooLarge {
            original_size,
            base64_size: base64.len(),
        });
    }

    Ok(ProcessedImage {
        base64,
        media_type,
        original_size,
        final_size,
    })
}

/// Resize image using external tool (sips on macOS, convert on Linux)
fn resize_image(data: &[u8], _format: ImageFormat) -> Result<Vec<u8>, ImageError> {
    // For now, we'll use a simple approach: try to resize via external tools
    // On macOS: sips
    // On Linux: ImageMagick convert

    // If external tools aren't available, just return the original data
    // and let the API handle it (may fail if too large)
    if data.len() <= IMAGE_TARGET_RAW_SIZE {
        return Ok(data.to_vec());
    }

    // Try to use external resizing tool
    let temp_dir = std::env::temp_dir();
    let input_path = temp_dir.join("aemeath_input_image");
    let output_path = temp_dir.join("aemeath_output_image.jpg");

    // Write input file
    std::fs::write(&input_path, data).map_err(|e| ImageError::WriteError(e.to_string()))?;

    let resize_result = resize_with_external_tool(&input_path, &output_path);

    // Cleanup input file
    std::fs::remove_file(&input_path).ok();

    resize_result?;

    // Read resized output
    let result = std::fs::read(&output_path).map_err(|e| ImageError::ReadError(e.to_string()))?;

    // Cleanup output file
    std::fs::remove_file(&output_path).ok();

    Ok(result)
}

#[cfg(target_os = "macos")]
fn resize_with_external_tool(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
) -> Result<(), ImageError> {
    let output_str = output_path
        .to_str()
        .ok_or_else(|| ImageError::ResizeError("output path is not valid UTF-8".to_string()))?;
    let input_str = input_path
        .to_str()
        .ok_or_else(|| ImageError::ResizeError("input path is not valid UTF-8".to_string()))?;
    let status = std::process::Command::new("sips")
        .args([
            "-s",
            "format",
            "jpeg",
            "-Z",
            &IMAGE_MAX_WIDTH.to_string(),
            "--out",
            output_str,
            input_str,
        ])
        .status()
        .map_err(|e| ImageError::ResizeError(e.to_string()))?;

    if !status.success() {
        return Err(ImageError::ResizeError("sips failed".to_string()));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn resize_with_external_tool(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
) -> Result<(), ImageError> {
    let input_str = input_path
        .to_str()
        .ok_or_else(|| ImageError::ResizeError("input path is not valid UTF-8".to_string()))?;
    let output_str = output_path
        .to_str()
        .ok_or_else(|| ImageError::ResizeError("output path is not valid UTF-8".to_string()))?;
    let status = std::process::Command::new("convert")
        .args([
            input_str,
            "-resize",
            &format!("{}x{}>", IMAGE_MAX_WIDTH, IMAGE_MAX_HEIGHT),
            "-quality",
            "80",
            output_str,
        ])
        .status()
        .map_err(|e| ImageError::ResizeError(e.to_string()))?;

    if !status.success() {
        return Err(ImageError::ResizeError("convert failed".to_string()));
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn resize_with_external_tool(
    _input_path: &std::path::Path,
    _output_path: &std::path::Path,
) -> Result<(), ImageError> {
    Err(ImageError::ResizeError(
        "no resize tool available on this platform".to_string(),
    ))
}

/// Base64 encode data
fn base64_encode(data: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(data)
}

#[derive(Debug)]
pub enum ImageError {
    FileNotFound(String),
    ReadError(String),
    WriteError(String),
    EmptyFile,
    TooLarge {
        original_size: usize,
        base64_size: usize,
    },
    ResizeError(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::FileNotFound(path) => write!(f, "Image file not found: {}", path),
            ImageError::ReadError(e) => write!(f, "Failed to read image: {}", e),
            ImageError::WriteError(e) => write!(f, "Failed to write temporary file: {}", e),
            ImageError::EmptyFile => write!(f, "Image file is empty"),
            ImageError::TooLarge {
                original_size,
                base64_size,
            } => {
                write!(
                    f,
                    "Image too large: {} bytes (base64: {} bytes). Max base64 size: {} bytes",
                    original_size, base64_size, API_IMAGE_MAX_BASE64_SIZE
                )
            }
            ImageError::ResizeError(e) => write!(f, "Failed to resize image: {}", e),
        }
    }
}

impl std::error::Error for ImageError {}

/// Read image data from system clipboard.
/// macOS: uses osascript to save clipboard to temp file, then reads it.
/// Linux: uses xclip -selection clipboard -t image/png -o
pub async fn read_clipboard_image() -> Result<ProcessedImage, ImageError> {
    let data = read_clipboard_image_bytes().await?;
    process_image_data(&data)
}

#[cfg(target_os = "macos")]
async fn read_clipboard_image_bytes() -> Result<Vec<u8>, ImageError> {
    use tokio::process::Command;

    let temp_path = std::env::temp_dir().join("aemeath_clipboard_image.png");
    let temp_str = temp_path.to_string_lossy().to_string();

    // Use osascript to check if clipboard has an image and save it
    let script = format!(
        r#"
        try
            set imgData to the clipboard as «class PNGf»
            set filePath to POSIX file "{temp_str}"
            set fileRef to open for access filePath with write permission
            set eof fileRef to 0
            write imgData to fileRef
            close access fileRef
            return "ok"
        on error
            try
                set imgData to the clipboard as TIFF picture
                set filePath to POSIX file "{temp_str}"
                set fileRef to open for access filePath with write permission
                set eof fileRef to 0
                write imgData to fileRef
                close access fileRef
                return "ok"
            on error errMsg
                return "error:" & errMsg
            end try
        end try
        "#
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .await
        .map_err(|e| ImageError::ReadError(format!("osascript failed: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if stdout.starts_with("error:") || !output.status.success() {
        // Cleanup
        tokio::fs::remove_file(&temp_path).await.ok();
        return Err(ImageError::ReadError(
            "No image in clipboard. Copy an image first.".to_string(),
        ));
    }

    let data = tokio::fs::read(&temp_path)
        .await
        .map_err(|e| ImageError::ReadError(format!("failed to read clipboard image: {e}")))?;

    // Cleanup
    tokio::fs::remove_file(&temp_path).await.ok();

    if data.is_empty() {
        return Err(ImageError::ReadError(
            "No image in clipboard. Copy an image first.".to_string(),
        ));
    }

    Ok(data)
}

#[cfg(target_os = "linux")]
async fn read_clipboard_image_bytes() -> Result<Vec<u8>, ImageError> {
    use tokio::process::Command;

    // Try xclip first
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "image/png", "-o"])
        .output()
        .await;

    if let Ok(output) = output {
        if output.status.success() && !output.stdout.is_empty() {
            return Ok(output.stdout);
        }
    }

    // Fallback to xsel
    let output = Command::new("xsel")
        .args(["--clipboard", "--output"])
        .output()
        .await;

    if let Ok(output) = output {
        if output.status.success() && !output.stdout.is_empty() {
            // Check if it's actually image data (magic bytes)
            let data = &output.stdout;
            if data.len() > 4
                && (data[0] == 0x89 || data[0] == 0xff || data[0] == 0x47 || data[0] == 0x52)
            {
                return Ok(output.stdout);
            }
        }
    }

    // Try wl-paste for Wayland
    let output = Command::new("wl-paste")
        .args(["--type", "image/png"])
        .output()
        .await;

    if let Ok(output) = output {
        if output.status.success() && !output.stdout.is_empty() {
            return Ok(output.stdout);
        }
    }

    Err(ImageError::ReadError(
        "No image in clipboard. Requires xclip, xsel, or wl-paste.".to_string(),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
async fn read_clipboard_image_bytes() -> Result<Vec<u8>, ImageError> {
    Err(ImageError::ReadError(
        "Clipboard image reading not supported on this platform.".to_string(),
    ))
}
