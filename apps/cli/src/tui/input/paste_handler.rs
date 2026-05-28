use crate::tui::app::event::UiEvent;
use tokio::sync::mpsc;

impl crate::tui::app::App {
    /// Handle paste events when not processing.
    pub(crate) fn handle_paste_event(&mut self, text: String, ui_tx: &mpsc::Sender<UiEvent>) {
        self.input.just_pasted = true;
        if text.trim().is_empty() {
            // Empty paste — try to read clipboard image
            let output_tx = ui_tx.clone();
            let Some(agent_client) = self.agent_client.clone() else {
                self.output_area
                    .push_system("[cannot read clipboard image without SDK client]");
                return;
            };
            tokio::spawn(async move {
                match agent_client.read_clipboard_image().await {
                    Ok(img) => {
                        let size = img.final_size;
                        let _ = output_tx.send(UiEvent::ClipboardImage(img)).await;
                        let _ = output_tx
                            .send(UiEvent::SystemMessage(format!(
                                "[clipboard image added ({} bytes). Type message to send.]",
                                size
                            )))
                            .await;
                    }
                    Err(e) => {
                        let _ = output_tx
                            .send(UiEvent::Error(format!("No image in clipboard: {e}")))
                            .await;
                    }
                }
            });
            self.output_area.push_system("[reading clipboard image...]");
        } else if is_image_path(text.trim()) {
            self.output_area
                .push_system(&format!("[loading image: {}...]", text.trim()));
            // We can't await here directly since this is a sync method,
            // so we'll handle image file loading via spawn
            let path = text.trim().to_string();
            let Some(agent_client) = self.agent_client.clone() else {
                self.output_area
                    .push_system("[cannot load image without SDK client]");
                return;
            };
            let tx = ui_tx.clone();
            tokio::spawn(async move {
                match agent_client.process_image_file(path).await {
                    Ok(img) => {
                        let size = img.final_size;
                        let _ = tx.send(UiEvent::ClipboardImage(img)).await;
                        let _ = tx
                            .send(UiEvent::SystemMessage(format!(
                                "[image loaded ({} bytes). Type message to send.]",
                                size
                            )))
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(UiEvent::Error(format!("Failed to load image: {e}")))
                            .await;
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
            // 同步模型状态：paste 直接修改 textarea 未走模型，
            // 下次按键（如空格）将触发 model.insert → TextChanged → set_text，
            // 用旧文本覆盖 textarea 已粘贴的内容（同 #77）。
            let text = self.input_area.get_text();
            self.model.input.document.clear();
            self.model.input.document.insert_text(&text);
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
