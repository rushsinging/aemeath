use super::{process_image_data, ImageError, ProcessedImage};

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

    let mut command = Command::new("osascript");
    command.arg("-e").arg(&script);
    utils::configure_tokio_noninteractive(&mut command)
        .map_err(|error| ImageError::ReadError(format!("osascript isolation failed: {error}")))?;
    let output = command
        .output()
        .await
        .map_err(|error| ImageError::ReadError(format!("osascript failed: {error}")))?;

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
    let mut command = Command::new("xclip");
    command.args(["-selection", "clipboard", "-t", "image/png", "-o"]);
    let output = if utils::configure_tokio_noninteractive(&mut command).is_ok() {
        command.output().await
    } else {
        return Err(ImageError::ReadError("clipboard command isolation unsupported".to_string()));
    };

    if let Ok(output) = output {
        if output.status.success() && !output.stdout.is_empty() {
            return Ok(output.stdout);
        }
    }

    // Fallback to xsel
    let mut command = Command::new("xsel");
    command.args(["--clipboard", "--output"]);
    let output = if utils::configure_tokio_noninteractive(&mut command).is_ok() {
        command.output().await
    } else {
        return Err(ImageError::ReadError("clipboard command isolation unsupported".to_string()));
    };

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
    let mut command = Command::new("wl-paste");
    command.args(["--type", "image/png"]);
    let output = if utils::configure_tokio_noninteractive(&mut command).is_ok() {
        command.output().await
    } else {
        return Err(ImageError::ReadError("clipboard command isolation unsupported".to_string()));
    };

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
