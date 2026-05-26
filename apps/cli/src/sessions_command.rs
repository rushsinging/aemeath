use std::sync::Arc;

/// 处理 `aemeath sessions` 子命令
pub(crate) async fn run_sessions_command(
    client: Arc<dyn sdk::AgentClient>,
    delete: Option<String>,
    json: bool,
    limit: usize,
) {
    if let Some(id) = delete {
        match client.delete_session(&id).await {
            Ok(()) => println!("Session {} deleted.", id),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    let sessions = client.list_sessions().await.unwrap_or_else(|e| {
        eprintln!("Error: {e}");
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
