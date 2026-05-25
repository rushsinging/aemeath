//! Memory command — manage persistent memory entries.

use super::memory_support::open_memory_store;
use crate::command::{Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult};
use crate::memory::{
    format_add_result, format_memory_list, parse_category, parse_layer, MemoryCategory,
    MemoryEntry, MemoryLayer, MemorySource,
};

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "memory".to_string(),
            "Manage persistent memory".to_string(),
            CommandCategory::Utility,
            memory_execute,
        )
        .with_usage(vec![
            "/memory - List project and global memory".to_string(),
            "/memory add <content> - Add project memory".to_string(),
            "/memory add --global --category decision <content>".to_string(),
            "/memory delete <id> - Delete memory".to_string(),
            "/memory pin <id> - Pin memory".to_string(),
            "/memory unpin <id> - Unpin memory".to_string(),
            "/memory search <query> - Search memory".to_string(),
            "/memory compact - Archive eviction candidates".to_string(),
            "/memory remind - Show current session reminders in TUI".to_string(),
            "/memory stats - Show memory statistics".to_string(),
        ])
        .with_aliases(vec!["mem".to_string()])
    })
}

fn memory_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    if !ctx.config.memory.enabled {
        return CommandResult::Error("Memory 系统已禁用。".to_string());
    }

    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let action = parts.next().unwrap_or("").to_lowercase();
    let rest = parts.next().unwrap_or("").trim();

    match action.as_str() {
        "" | "list" => list_memory(ctx),
        "add" => add_memory(rest, ctx),
        "delete" | "del" | "remove" | "rm" => delete_memory(rest, ctx),
        "pin" => set_pin(rest, true, ctx),
        "unpin" => set_pin(rest, false, ctx),
        "search" => search_memory(rest, ctx),
        "compact" => compact_memory(ctx),
        "stats" => stats_memory(ctx),
        "remind" | "reminder" | "reminders" => CommandResult::Success(
            "当前会话 reminders 请在 TUI 中使用 /memory remind 查看。".to_string(),
        ),
        _ => CommandResult::Error(format!("未知 memory 子命令: {action}")),
    }
}

fn list_memory(ctx: &CommandContext) -> CommandResult {
    let store = match open_memory_store(ctx) {
        Ok(store) => store,
        Err(error) => return CommandResult::Error(error),
    };

    match store.list(None) {
        Ok(entries) => CommandResult::Success(format_memory_list(&entries)),
        Err(error) => CommandResult::Error(error.to_string()),
    }
}

fn add_memory(args: &str, ctx: &CommandContext) -> CommandResult {
    let (layer, category, content) = match parse_add_args(args) {
        Ok(parsed) => parsed,
        Err(error) => return CommandResult::Error(error),
    };
    let mut store = match open_memory_store(ctx) {
        Ok(store) => store,
        Err(error) => return CommandResult::Error(error),
    };
    let entry = MemoryEntry::new(layer, category, content, MemorySource::User)
        .with_source_ref(ctx.session_id.clone());

    match store.add(entry) {
        Ok(result) => CommandResult::Success(format_add_result(result)),
        Err(error) => CommandResult::Error(error.to_string()),
    }
}

fn delete_memory(id: &str, ctx: &CommandContext) -> CommandResult {
    if id.trim().is_empty() {
        return CommandResult::Error("用法: /memory delete <id>".to_string());
    }
    let mut store = match open_memory_store(ctx) {
        Ok(store) => store,
        Err(error) => return CommandResult::Error(error),
    };

    match store.delete(id.trim()) {
        Ok(()) => CommandResult::Success("记忆已删除。".to_string()),
        Err(error) => CommandResult::Error(error.to_string()),
    }
}

fn set_pin(id: &str, pinned: bool, ctx: &CommandContext) -> CommandResult {
    if id.trim().is_empty() {
        return CommandResult::Error("用法: /memory pin <id>".to_string());
    }
    let mut store = match open_memory_store(ctx) {
        Ok(store) => store,
        Err(error) => return CommandResult::Error(error),
    };

    match store.pin(id.trim(), pinned) {
        Ok(()) if pinned => CommandResult::Success("记忆已固定。".to_string()),
        Ok(()) => CommandResult::Success("记忆已取消固定。".to_string()),
        Err(error) => CommandResult::Error(error.to_string()),
    }
}

fn search_memory(query: &str, ctx: &CommandContext) -> CommandResult {
    if query.trim().is_empty() {
        return CommandResult::Error("用法: /memory search <query>".to_string());
    }
    let store = match open_memory_store(ctx) {
        Ok(store) => store,
        Err(error) => return CommandResult::Error(error),
    };

    match store.search(query.trim(), 20) {
        Ok(entries) => CommandResult::Success(format_memory_list(&entries)),
        Err(error) => CommandResult::Error(error.to_string()),
    }
}

fn compact_memory(ctx: &CommandContext) -> CommandResult {
    let mut store = match open_memory_store(ctx) {
        Ok(store) => store,
        Err(error) => return CommandResult::Error(error),
    };

    match store.compact() {
        Ok(result) => CommandResult::Success(format!(
            "Memory compact 完成：归档 {} 条，剩余 {} 条。",
            result.archived, result.remaining
        )),
        Err(error) => CommandResult::Error(error.to_string()),
    }
}

fn stats_memory(ctx: &CommandContext) -> CommandResult {
    let store = match open_memory_store(ctx) {
        Ok(store) => store,
        Err(error) => return CommandResult::Error(error),
    };

    match store.stats(0) {
        Ok(stats) => CommandResult::Success(format!(
            "Memory Stats:\n  Global: {}\n  Global archive: {}\n  Project: {}\n  Project archive: {}\n  Reminders: {}",
            stats.global_count,
            stats.global_archive_count,
            stats.project_count,
            stats.project_archive_count,
            stats.reminders_count
        )),
        Err(error) => CommandResult::Error(error.to_string()),
    }
}

fn parse_add_args(args: &str) -> Result<(MemoryLayer, MemoryCategory, String), String> {
    let mut layer = MemoryLayer::Project;
    let mut category = MemoryCategory::Fact;
    let mut content_parts = Vec::new();
    let mut iter = args.split_whitespace();

    while let Some(part) = iter.next() {
        match part {
            "--global" => layer = MemoryLayer::Global,
            "--project" => layer = MemoryLayer::Project,
            "--layer" => {
                let value = iter.next().ok_or_else(|| "--layer 需要参数".to_string())?;
                layer = parse_layer(value).ok_or_else(|| format!("无效 memory layer: {value}"))?;
            }
            "--category" | "--cat" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--category 需要参数".to_string())?;
                category = parse_category(value)
                    .ok_or_else(|| format!("无效 memory category: {value}"))?;
            }
            other => content_parts.push(other),
        }
    }

    let content = content_parts.join(" ");
    if content.trim().is_empty() {
        return Err("用法: /memory add [--global|--project] [--category fact|decision|preference|pattern|pitfall] <content>".to_string());
    }
    Ok((layer, category, content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_add_args_default() {
        let (layer, category, content) = parse_add_args("记住这个事实").unwrap();

        assert_eq!(layer, MemoryLayer::Project);
        assert_eq!(category, MemoryCategory::Fact);
        assert_eq!(content, "记住这个事实");
    }

    #[test]
    fn test_parse_add_args_global_decision() {
        let (layer, category, content) =
            parse_add_args("--global --category decision 使用中文回复").unwrap();

        assert_eq!(layer, MemoryLayer::Global);
        assert_eq!(category, MemoryCategory::Decision);
        assert_eq!(content, "使用中文回复");
    }

    #[test]
    fn test_parse_add_args_empty_error() {
        let result = parse_add_args("--global --category fact");

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_add_args_invalid_layer_error() {
        let result = parse_add_args("--layer session 内容");

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_add_args_invalid_category_error() {
        let result = parse_add_args("--category context 内容");

        assert!(result.is_err());
    }
}
