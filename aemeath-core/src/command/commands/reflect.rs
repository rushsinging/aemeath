//! Reflect command — inspect memory and produce a lightweight reflection report.

use super::memory_support::open_memory_store;
use crate::command::{Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult};
use crate::memory::{MemoryEntry, MemoryLayer};
use crate::reflection::{ReflectionEngine, ReflectionOutput};

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "reflect".to_string(),
            "Run lightweight reflection over current memory".to_string(),
            CommandCategory::Utility,
            reflect_execute,
        )
        .with_usage(vec![
            "/reflect - Show reflection report".to_string(),
            "/reflect apply - Apply pending reflection suggestions (placeholder)".to_string(),
        ])
    })
}

fn reflect_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    if !ctx.config.memory.reflection.enabled {
        return CommandResult::Error("Reflection 系统已禁用。".to_string());
    }

    match args.trim() {
        "" => run_reflection(ctx),
        "apply" => CommandResult::Success("暂无待应用的 Reflection 建议。".to_string()),
        "stats" | "history" => CommandResult::Success("Reflection stats/history 将在打磨阶段支持。".to_string()),
        other => CommandResult::Error(format!("未知 reflect 子命令: {other}")),
    }
}

fn run_reflection(ctx: &CommandContext) -> CommandResult {
    let store = match open_memory_store(ctx) {
        Ok(store) => store,
        Err(error) => return CommandResult::Error(error),
    };
    let memories = match store.list(Some(MemoryLayer::Project)) {
        Ok(memories) => memories,
        Err(error) => return CommandResult::Error(error.to_string()),
    };

    let output = build_lightweight_output(&memories);
    CommandResult::Success(ReflectionEngine::format_output(&output))
}

fn build_lightweight_output(memories: &[MemoryEntry]) -> ReflectionOutput {
    let mut deviations = Vec::new();
    if memories.is_empty() {
        deviations.push("当前项目没有长期记忆，建议在关键决策后写入 Memory。".to_string());
    }
    let outdated_memories = memories
        .iter()
        .filter(|entry| entry.outdated)
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();

    ReflectionOutput {
        deviations,
        suggested_memories: Vec::new(),
        outdated_memories,
        user_alert: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryCategory, MemorySource};

    #[test]
    fn test_build_lightweight_output_empty_memory() {
        let output = build_lightweight_output(&[]);

        assert_eq!(output.deviations.len(), 1);
        assert!(output.suggested_memories.is_empty());
    }

    #[test]
    fn test_build_lightweight_output_normal_memory() {
        let entry = MemoryEntry::new(
            MemoryLayer::Project,
            MemoryCategory::Decision,
            "使用 Memory 注入系统提示",
            MemorySource::User,
        );
        let output = build_lightweight_output(&[entry]);

        assert!(output.deviations.is_empty());
        assert!(output.outdated_memories.is_empty());
    }

    #[test]
    fn test_build_lightweight_output_outdated_memory() {
        let mut entry = MemoryEntry::new(
            MemoryLayer::Project,
            MemoryCategory::Decision,
            "旧决策",
            MemorySource::User,
        );
        entry.outdated = true;
        let id = entry.id.clone();
        let output = build_lightweight_output(&[entry]);

        assert_eq!(output.outdated_memories, vec![id]);
    }
}
