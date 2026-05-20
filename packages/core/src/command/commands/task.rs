//! Task lifecycle command — manage task batch pause/resume/drop/history.
//!
//! Registered via `inventory::submit!` for compile-time collection.
//!
//! Feature #25: Task list 跨轮次生命周期策略

use crate::command::{Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult};
use crate::task::{BatchStatus, TaskStore};
use std::sync::Arc;

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new_async(
            "task".to_string(),
            "Manage task lifecycle".to_string(),
            CommandCategory::Tasks,
            |args: String, ctx: &mut CommandContext| {
                let task_store = ctx.task_store.clone();
                Box::pin(async move {
                    task_execute(args, task_store).await
                })
            },
        )
        .with_usage(vec![
            "/task pause - Pause the current batch".to_string(),
            "/task resume [batch_id] - Resume a paused batch".to_string(),
            "/task keep - Keep the current batch".to_string(),
            "/task drop - Drop unfinished tasks in the current batch".to_string(),
            "/task history - List all batches in this session".to_string(),
        ])
    })
}

async fn task_execute(args: String, task_store: Option<Arc<TaskStore>>) -> CommandResult {
    let Some(task_store) = task_store else {
        return CommandResult::Error("No task store available".to_string());
    };

    let mut parts = args.split_whitespace();
    let subcommand = parts.next().unwrap_or("");

    match subcommand {
        "pause" => {
            let current_batch = task_store
                .list_current_batch()
                .await
                .into_iter()
                .map(|task| task.batch)
                .max();

            let Some(batch_id) = current_batch else {
                return CommandResult::Error("No current batch available".to_string());
            };

            task_store.get_or_create_batch(batch_id).await;
            task_store
                .set_batch_status(batch_id, BatchStatus::Paused)
                .await;

            CommandResult::Success(format!("Batch #{} paused", batch_id))
        }

        "resume" => {
            let batch_id = match parts.next() {
                Some(raw) => match raw.parse::<u64>() {
                    Ok(id) => id,
                    Err(_) => {
                        return CommandResult::Error(format!("Invalid batch id: {}", raw));
                    }
                },
                None => {
                    // Resume the most recently paused batch
                    let mut paused_batches: Vec<_> = task_store
                        .list_batches()
                        .await
                        .into_iter()
                        .filter(|batch| batch.status == BatchStatus::Paused)
                        .collect();

                    paused_batches.sort_by_key(|batch| batch.id);

                    let Some(batch) = paused_batches.pop() else {
                        return CommandResult::Error("No paused batch available".to_string());
                    };

                    batch.id
                }
            };

            let Some(batch) = task_store.get_batch(batch_id).await else {
                return CommandResult::Error(format!("Batch #{} not found", batch_id));
            };

            if batch.status != BatchStatus::Paused {
                return CommandResult::Error(format!(
                    "Batch #{} is not paused (status: {:?})",
                    batch_id, batch.status
                ));
            }

            task_store
                .set_batch_status(batch_id, BatchStatus::Active)
                .await;
            task_store.reset_silence(batch_id).await;

            CommandResult::Success(format!("Batch #{} resumed", batch_id))
        }

        "keep" => {
            let current_batch = task_store
                .list_current_batch()
                .await
                .into_iter()
                .map(|task| task.batch)
                .max();

            let Some(batch_id) = current_batch else {
                return CommandResult::Error("No current batch available".to_string());
            };

            task_store.get_or_create_batch(batch_id).await;
            task_store
                .set_batch_status(batch_id, BatchStatus::Active)
                .await;
            task_store.reset_silence(batch_id).await;

            CommandResult::Success(format!("Batch #{} kept active", batch_id))
        }

        "drop" => {
            let current_batch = task_store
                .list_current_batch()
                .await
                .into_iter()
                .map(|task| task.batch)
                .max();

            let Some(batch_id) = current_batch else {
                return CommandResult::Error("No current batch available".to_string());
            };

            let incomplete = task_store.incomplete_count(batch_id).await;
            task_store.cancel_batch(batch_id).await;

            CommandResult::Success(format!(
                "Batch #{} dropped ({} incomplete tasks cancelled)",
                batch_id, incomplete
            ))
        }

        "history" => {
            let mut batches = task_store.list_batches().await;
            batches.sort_by_key(|batch| batch.id);

            if batches.is_empty() {
                return CommandResult::Success("No task batches".to_string());
            }

            let mut lines = Vec::new();

            for batch in batches {
                let tasks = task_store
                    .tasks_in_batch(
                        batch.id,
                        &[
                            crate::task::TaskStatus::Pending,
                            crate::task::TaskStatus::InProgress,
                            crate::task::TaskStatus::Completed,
                        ],
                    )
                    .await;

                let total = tasks.len();
                let incomplete = task_store.incomplete_count(batch.id).await;

                lines.push(format!(
                    "Batch #{}: {} ({} tasks, {} incomplete)",
                    batch.id,
                    batch_status_label(batch.status),
                    total,
                    incomplete
                ));
            }

            CommandResult::Success(lines.join("\n"))
        }

        "" => CommandResult::Error("Usage: /task <pause|resume|keep|drop|history>".to_string()),

        other => CommandResult::Error(format!("Unknown task command: {}", other)),
    }
}

fn batch_status_label(status: BatchStatus) -> &'static str {
    match status {
        BatchStatus::Active => "Active",
        BatchStatus::Paused => "Paused",
        BatchStatus::Archived => "Archived",
    }
}
