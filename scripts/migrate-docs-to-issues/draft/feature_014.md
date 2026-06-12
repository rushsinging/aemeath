<!-- Migrated from: docs/feature/archived/014-session-id-uuidv7.md -->
# #14 Session ID 自增无冲突方案

**归档日期**：2026-05-01

**实现**：采用 UUIDv7 替换原先 `{timestamp_ms_hex}{rand_u32}` 24 位 hex ID。

- 新 ID 格式：标准 UUIDv7，例如 `018f2d4e-9c7a-7b12-9a34-8f0c1d2e3f45`
- UUIDv7 前缀携带毫秒时间戳，字典序≈创建时间序
- 通过 `Uuid::new_v7(Timestamp::now(NoContext))` 生成，依赖 `uuid` crate 的 `v7` feature
- 不引入 `session_counter.json` / lock 目录，避免本地状态损坏、锁残留、时钟回拨等复杂度
- 旧格式 24 位 hex ID 不迁移、不重命名，`validate_session_id` 仍然接受，可继续 `--resume`

**未纳入**：用户可见短编号（如 `/resume 5`）需要独立 session index 与命令解析改造，本期不实现，避免扩大迁移面。

**修复 commit**：e80db01 `feat: Session ID 改用 UUIDv7 替换随机 hex ID`

**涉及文件**：
- `aemeath-core/src/state.rs`（`new_session_id` 改用 UUIDv7）
- `aemeath-core/src/session.rs`（`validate_session_id` 兼容新旧两种 ID）
- `Cargo.toml`（`uuid` crate `v7` feature）

**测试覆盖**：
- 正常路径：新建 ID 是合法 UUIDv7 且通过校验
- 边界条件：同一 timestamp 下连续生成两个 UUIDv7 仍不重复
- 错误路径：包含路径分隔符的伪 UUID 被 `validate_session_id` 拒绝
- 兼容路径：旧 24 位 hex session ID 仍通过校验
