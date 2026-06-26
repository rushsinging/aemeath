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
                self.append_system_notice("[cannot read clipboard image without SDK client]");
                return;
            };
            crate::tui::effect::spawn_guard::spawn_guarded("clipboard_image", async move {
                match agent_client.read_clipboard_image().await {
                    Ok(img) => {
                        // #fix-tui-image-input-output：图片直接作为 span 插入输入区（带 [Image #N]
                        // 占位符），不再发「添加成功」banner，由插入的占位 span 提示用户
                        let _ = output_tx.send(UiEvent::ClipboardImage(img)).await;
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
            let Some(agent_client) = self.agent_client.clone() else {
                self.append_system_notice("[cannot load image without SDK client]");
                return;
            };
            let tx = ui_tx.clone();
            crate::tui::effect::spawn_guard::spawn_guarded("image_file", async move {
                match agent_client.process_image_file(path).await {
                    Ok(img) => {
                        // #fix-tui-image-input-output：图片作为 span 插入输入区
                        let _ = tx.send(UiEvent::ClipboardImage(img)).await;
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
