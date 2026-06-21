use crate::tui::app::App;

impl App {
    pub(super) fn handle_memory_list(&mut self, reminders: &[sdk::ReminderView]) {
        if reminders.is_empty() {
            self.append_system_notice("当前没有 session reminder。");
            return;
        }
        let mut text = String::from("Session Reminders:");
        for r in reminders {
            let marker = if r.done { "✓" } else { "□" };
            text.push_str(&format!("\n{marker} {} {}", r.id, r.content));
        }
        self.append_system_notice(text);
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::app::App;
    use std::path::PathBuf;

    fn make_app() -> App {
        App::new(
            "sess-rem".to_string(),
            PathBuf::from("/tmp"),
            "m".to_string(),
        )
    }

    fn system_texts(app: &App) -> Vec<String> {
        app.model
            .conversation
            .timeline
            .items()
            .iter()
            .filter_map(|item| match item {
                crate::tui::model::output_timeline::OutputTimelineItem::System { text, .. } => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_handle_memory_list_empty_shows_no_reminder_notice() {
        let mut app = make_app();
        app.handle_memory_list(&[]);
        assert!(system_texts(&app)
            .iter()
            .any(|text| text == "当前没有 session reminder。"));
    }

    #[test]
    fn test_handle_memory_list_with_items_lists_each_reminder() {
        let mut app = make_app();
        let reminders = vec![
            sdk::ReminderView {
                id: "r1".to_string(),
                content: "写测试".to_string(),
                done: false,
                created_at: 0,
            },
            sdk::ReminderView {
                id: "r2".to_string(),
                content: "提交".to_string(),
                done: true,
                created_at: 0,
            },
        ];
        app.handle_memory_list(&reminders);
        let texts = system_texts(&app);
        let combined = texts.join("\n");
        assert!(combined.contains("Session Reminders:"));
        assert!(combined.contains("□ r1 写测试"));
        assert!(combined.contains("✓ r2 提交"));
    }

    #[test]
    fn test_handle_memory_list_single_reminder_creates_block() {
        let mut app = make_app();
        let reminders = vec![sdk::ReminderView {
            id: "only".to_string(),
            content: "x".to_string(),
            done: false,
            created_at: 0,
        }];
        app.handle_memory_list(&reminders);
        // 启动横幅占用 BANNER_LINES.len() 个 System block，reminder 再追加一个。
        let banner = crate::tui::model::conversation::notice::BANNER_LINES.len();
        assert_eq!(system_texts(&app).len(), banner + 1);
    }
}
