//! MemoryPort — Memory BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! PL 类型细化由 #895 负责；此处只定义最小骨架。
//! 检索质量（BM25/语义检索/评分）归 #547。

// ─── Published Language（最小骨架，#895 迁移到 memory crate） ───

/// Memory 检索查询。
// TODO(#895): 迁移到 memory crate 并细化字段。
#[derive(Debug, Clone)]
pub struct MemoryQuery {
    /// 查询文本。
    pub text: String,
    /// 最大返回条数。
    pub limit: usize,
}

/// Memory 条目。
// TODO(#895): 迁移到 memory crate 并细化字段。
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    /// 记忆内容。
    pub content: String,
    /// 相关性分数（越高越相关）。
    pub score: f64,
}

// ─── Port trait ───

/// Memory BC 的出站端口。
///
/// Sub Run 使用 `NoOpMemory`（不读不写）。
pub trait MemoryPort: Send + Sync {
    /// 检索与查询相关的记忆。
    fn retrieve(&self, query: &MemoryQuery) -> Vec<MemoryEntry>;

    /// 写入一条记忆。
    fn write(&self, entry: MemoryEntry);
}
