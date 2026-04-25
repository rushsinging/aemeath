use super::UiEvent;
use tokio::sync::mpsc;

impl super::App {
    /// Handle paste events when not processing.
    pub(super) fn handle_paste_event(
        &mut self,
        text: String,
        ui_tx: &mpsc::Sender<UiEvent>,
    ) {
        self.just_pasted = true;
        if text.trim().is_empty() {
            // Empty paste — try to read clipboard image
            let output_tx = ui_tx.clone();
            tokio::spawn(async move {
                match crate::image::read_clipboard_image().await {
                    Ok(img) => {
                        let size = img.final_size;
                        let _ = output_tx.send(UiEvent::ClipboardImage(img)).await;
                        let _ = output_tx.send(UiEvent::SystemMessage(
                            format!("[clipboard image added ({} bytes). Type message to send.]", size)
                        )).await;
                    }
                    Err(e) => {
                        let _ = output_tx.send(UiEvent::Error(
                            format!("No image in clipboard: {e}")
                        )).await;
                    }
                }
            });
            self.output_area.push_system("[reading clipboard image...]");
        } else if crate::image::is_image_file(text.trim()) {
            self.output_area.push_system(&format!("[loading image: {}...]", text.trim()));
            // We can't await here directly since this is a sync method,
            // so we'll handle image file loading via spawn
            let path = text.trim().to_string();
            let tx = ui_tx.clone();
            tokio::spawn(async move {
                match crate::image::process_image_file(&path).await {
                    Ok(img) => {
                        let size = img.final_size;
                        let _ = tx.send(UiEvent::ClipboardImage(img)).await;
                        let _ = tx.send(UiEvent::SystemMessage(
                            format!("[image loaded ({} bytes). Type message to send.]", size)
                        )).await;
                    }
                    Err(e) => {
                        let _ = tx.send(UiEvent::Error(format!("Failed to load image: {e}"))).await;
                    }
                }
            });
        } else {
            for ch in text.chars() {
                if ch == '\n' || ch == '\r' {
                    self.input_area.enter(true);
                } else {
                    self.input_area.input(ch);
                }
            }
            self.update_suggestions();
        }
    }
}
