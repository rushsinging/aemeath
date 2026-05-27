use share::memory::{memory_base_dir, project_hash, MemoryStore};

pub fn open_memory_store(ctx: &crate::core::command::CommandContext) -> Result<MemoryStore, String> {
    MemoryStore::new(
        memory_base_dir(),
        project_hash(&ctx.cwd),
        ctx.config.memory.max_entries,
        ctx.config.memory.similarity_threshold,
    )
    .map_err(|error| error.to_string())
}
