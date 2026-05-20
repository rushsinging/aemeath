use crate::image::{is_image_file, process_image_file, ProcessedImage};
use std::path::Path;

/// Extract image file paths from user input text.
/// Returns (cleaned text with paths removed, list of processed images).
/// Recognizes patterns like:
///   - Bare paths: /path/to/image.png, ./screenshot.jpg
///   - Bracketed: [Image: /path/to/image.png]
///   - Tilde paths: ~/Desktop/photo.png
pub(crate) async fn extract_image_paths(input: &str, cwd: &Path) -> (String, Vec<ProcessedImage>) {
    let mut images = Vec::new();
    let mut clean = input.to_string();

    // Regex-free approach: split by whitespace, check each token
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut paths_found: Vec<String> = Vec::new();

    for token in &tokens {
        // Strip surrounding brackets, quotes, parens
        let stripped = token.trim_matches(|c| {
            c == '[' || c == ']' || c == '(' || c == ')' || c == '"' || c == '\''
        });

        if !is_image_file(stripped) {
            continue;
        }

        // Resolve the path
        let resolved = if stripped.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                home.join(&stripped[2..]).to_string_lossy().to_string()
            } else {
                continue;
            }
        } else if Path::new(stripped).is_absolute() {
            stripped.to_string()
        } else {
            cwd.join(stripped).to_string_lossy().to_string()
        };

        if !Path::new(&resolved).exists() {
            continue;
        }

        match process_image_file(&resolved).await {
            Ok(img) => {
                println!("[auto-attached image ({} bytes)]", img.original_size);
                images.push(img);
                paths_found.push(token.to_string());
            }
            Err(e) => {
                eprintln!("[warning: failed to load image: {}]", e);
            }
        }
    }

    // Remove found paths from the text
    for path in &paths_found {
        clean = clean.replace(path, "").trim().to_string();
    }

    // Clean up leftover bracket artifacts like "[]" or "[Image: ]"
    clean = clean
        .replace("[Image: ]", "")
        .replace("[Image:]", "")
        .replace("[]", "");
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");

    (clean, images)
}
