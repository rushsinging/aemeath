<!-- Migrated from: docs/feature/archived/061-ddd-architecture-debt-closure.md -->
# Feature #61：架构债务收口（047 DDD 软约束落实）

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 归档日期 | 2026-06-02 |
| 状态 | 已确认完成 |
| 实现 | d83d516, 9de50fa, b1dda7b, aa9f0d3 |

## 背景

047 DDD spec 的硬约束已通过架构 guard 固化，但软约束仍存在技术债：`runtime::api` 整体转发、`share` 超出最小共享内核、supporting domain public API 暴露过宽、多个 domain 未按 COLA 分层。

## 完成内容

1. D1：收口 `runtime::api`，避免整体转发下游 crate 和内部实现。
2. D2：`share` 回归最小共享内核，迁出 ToolRegistry、TaskStore、worktree working path 行为、skill loader IO 等非共享职责。
3. D3：收窄 supporting domain Public API，跨 domain 访问统一走 `<crate>::api::*`。
4. D4：policy/project/storage/hook/prompt/tools/provider 等 supporting domain 完成内部 COLA 分层。
5. 新增 cross-crate api-boundary guard 与 share minimal kernel guard，防止架构回归。
6. 原 D5 audit / D6 policy 属新功能，拆出至 feature #62，不纳入本债务收口。

## 验证

- `cargo clean && cargo test --workspace` 通过。
- `cargo clippy --workspace -- -D warnings` 通过。
- `.agents/hooks/check-architecture-guards.sh` 通过。
- 用户确认完成。
