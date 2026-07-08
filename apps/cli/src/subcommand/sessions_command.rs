use std::sync::Arc;

/// 从 ChatStream 中等待 `SessionList` 事件，返回会话列表。
/// 流关闭时返回 None。
async fn wait_for_session_list(stream: &mut sdk::ChatStream) -> Option<Vec<sdk::SessionSummary>> {
    while let Some(event) = stream.recv().await {
        if let sdk::ChatEvent::SessionList { sessions } = event {
            return Some(sessions);
        }
    }
    None
}

/// 处理 `aemeath sessions` 子命令

pub(crate) async fn run_sessions_command(
    client: Arc<dyn sdk::AgentClient>,
    delete: Option<String>,
    json: bool,
    limit: usize,
) {
    // #567：delete_session / list_sessions 已从 trait 删除，改为通过 chat() 事件流请求。
    let (input_tx, input_port) =
        crate::tui::effect::session::processing::TuiInputEventPort::channel();
    let mut stream = client
        .chat(sdk::ChatRequest {
            user_input: None,
            queue_drain: None,
            input_events: Some(std::sync::Arc::new(input_port)),
        })
        .await
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });

    if let Some(id) = &delete {
        let _ = input_tx.send(sdk::ChatInputEvent::ManageSession {
            args: format!("delete {}", id),
        });
        // 等待会话列表回传（删除后 runtime 回传更新后的列表）。
        let sessions = wait_for_session_list(&mut stream).await.unwrap_or_else(|| {
            eprintln!("Error: stream closed before session list received");
            std::process::exit(1);
        });
        // 删除成功后检查列表中是否还有该 id。
        if sessions.iter().any(|s| &s.id == id) {
            eprintln!("Error: failed to delete session {}", id);
            std::process::exit(1);
        }
        println!("Session {} deleted.", id);
        return;
    }

    let _ = input_tx.send(sdk::ChatInputEvent::ManageSession {
        args: String::new(),
    });
    let sessions = wait_for_session_list(&mut stream).await.unwrap_or_else(|| {
        eprintln!("Error: stream closed before session list received");
        std::process::exit(1);
    });
    if sessions.is_empty() {
        println!("No saved sessions.");
        return;
    }

    let display: Vec<_> = sessions.into_iter().take(limit).collect();

    if json {
        let output: Vec<serde_json::Value> = display
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "title": s.title,
                    "project": s.project,
                    "model": s.model,
                    "messages": s.message_count,
                    "created_at": s.created_at,
                    "updated_at": s.updated_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        let header = ("ID", "SUMMARY", "PROJECT", "MSG", "UPDATED");
        let rows: Vec<(&str, String, &str, usize, &str)> = display
            .iter()
            .map(|s| {
                let summary_display: String = s.summary.chars().take(80).collect();
                let project = s.project.as_deref().unwrap_or("-");
                let updated = s.updated_at.get(..16).unwrap_or(&s.updated_at);
                (
                    s.id.as_str(),
                    summary_display,
                    project,
                    s.message_count,
                    updated,
                )
            })
            .collect();

        let w0 = rows
            .iter()
            .map(|r| r.0.len())
            .chain(std::iter::once(header.0.len()))
            .max()
            .unwrap_or(0);
        let w1 = rows
            .iter()
            .map(|r| r.1.len())
            .chain(std::iter::once(header.1.len()))
            .max()
            .unwrap_or(0)
            .min(60);
        let w2 = rows
            .iter()
            .map(|r| r.2.len())
            .chain(std::iter::once(header.2.len()))
            .max()
            .unwrap_or(0);

        println!(
            "{:<w$}  {:<w2$}  {:<w3$}  {:>3}  {}",
            header.0,
            header.1,
            header.2,
            header.3,
            header.4,
            w = w0,
            w2 = w1,
            w3 = w2
        );
        for (id, summary, project, msg, updated) in &rows {
            println!(
                "{:<w$}  {:<w2$}  {:<w3$}  {:>3}  {}",
                id,
                summary,
                project,
                msg,
                updated,
                w = w0,
                w2 = w1,
                w3 = w2
            );
        }
    }
}
