use crate::tui::app::event::UiEvent;
use tokio::sync::mpsc;

impl crate::tui::app::App {
    /// Handle paste events when not processing.
    pub(crate) fn handle_paste_event(&mut self, text: String, ui_tx: &mpsc::Sender<UiEvent>) {
        self.input.just_pasted = true;
        if text.trim().is_empty() {
            // Empty paste — try to read clipboard image
            let output_tx = ui_tx.clone();
            crate::tui::effect::spawn_guard::spawn_guarded("clipboard_image", async move {
                match crate::tui::render::input::clipboard::read_image().await {
                    Ok(img) => {
                        use base64::Engine;
                        let view = sdk::ClipboardImageView {
                            base64: base64::engine::general_purpose::STANDARD.encode(&img.data),
                            media_type: img.media_type,
                            final_size: img.data.len(),
                            display_path: None,
                            width: None,
                            height: None,
                        };
                        let _ = output_tx.send(UiEvent::ClipboardImage(view)).await;
                    }
                    Err(e) => {
                        let _ = output_tx
                            .send(UiEvent::Error(format!("No image in clipboard: {e}")))
                            .await;
                    }
                }
            });
            // 删：[reading clipboard image...] —— 进程提示已无意义（#fix-tui-image-input-output）
        } else if is_image_path(text.trim()) {
            // 删：[loading image: ...] —— 同上（#fix-tui-image-input-output）
            // We can't await here directly since this is a sync method,
            // so we'll handle image file loading via spawn
            let path = text.trim().to_string();
            let tx = ui_tx.clone();
            crate::tui::effect::spawn_guard::spawn_guarded("image_file", async move {
                match crate::tui::render::input::clipboard::process_image_file(&path) {
                    Ok(img) => {
                        use base64::Engine;
                        let view = sdk::ClipboardImageView {
                            base64: base64::engine::general_purpose::STANDARD.encode(&img.data),
                            media_type: img.media_type,
                            final_size: img.data.len(),
                            display_path: Some(path),
                            width: None,
                            height: None,
                        };
                        let _ = tx.send(UiEvent::ClipboardImage(view)).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(UiEvent::Error(format!("Failed to load image: {e}")))
                            .await;
                    }
                }
            });
        } else {
            self.handle_input_intent(
                crate::tui::model::input::intent::InputIntent::InsertPastedText(text),
            );
            self.update_suggestions();
        }
    }
}

fn is_image_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}
