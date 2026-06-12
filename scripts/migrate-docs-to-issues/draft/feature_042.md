<!-- Migrated from: docs/feature/active.md#42 -->
### #42 权限管控系统

**状态**：设计中

**目标**：AllowAll 模式下 Glob/Grep 访问 workspace 外路径仍被拦截。升级为完整权限管控：交互式授权 + 统一 `PermissionEngine`（action/resource/risk → Allow/Ask/Deny）；权限模式 AskMe/Auto/Plan/AllowAll。详见 [spec](specs/042-permission-control-system.md)。

**范围**：
1. `PermissionEngine` + 统一权限模型
2. 外部路径授权 → TUI 交互式选择
3. `ToolContext` 保存 session 级授权 scope
4. Read/Glob/Grep 先接入；Edit/Write 后续接入
5. AllowAll 下允许 workspace 内外读写，仅审计

**涉及路径**：`permission.rs`、`tool.rs`、`path_security.rs`、`file_read/glob/grep/file_edit/file_write.rs`、TUI `permissions.rs`/`tools.rs`

---
