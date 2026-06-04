# Bug #85：Ollama provider 声明但工厂未接线（整模块死代码）

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认修复 |
| 修复 | 111393e, de5a042, ba09882 |

## 症状

1. provider crate 中的 `OllamaProvider` 是完整的 `LlmProvider` 实现；但主启动路径 `main.rs` 和 pool 路径 `pool.rs` 的 `ApiDriverKind` 匹配分支缺少 `Ollama` case，导致即使配置了 `api_type: "ollama"`，实际运行时仍走 OpenAI 兼容分支（完全无视 `OllamaProvider` 模块）。
2. 运行时 `/model` 切换路径中配置 `api_type:"ollama"` 时会生成 `OpenAIProviderConfig` 并走 OpenAI-compatible API，进一步加剧问题。

## 根因

- `main.rs` 的 `build_client` 和 `build_pool` 未处理 `ApiDriverKind::Ollama`
- `openai_config.rs` 中 Ollama 没有被排除在 openai 配置路径之外
- 运行时 `/model` 切换路径也缺失 Ollama 分支

## 修复

1. `main.rs` 补 `ApiDriverKind::Ollama` → 构造 `OllamaProvider` 的分支
2. `pool.rs` 补 `ApiDriverKind::Ollama` 分支
3. `openai_config.rs` 在生成 `OpenAIProviderConfig` 时排除 Ollama
4. 运行时 `/model` 切换路径补 Ollama 分支，避免 `api_type:"ollama"` 仍生成 OpenAI 配置

## 验证

- `cargo check -p runtime`
- `cargo check -p provider`
- `cargo test -p runtime`
- `cargo test -p provider`
- 用户确认修复。

## 涉及路径

- `agent/features/runtime/core/bootstrap/{main,pool}.rs`
- `agent/features/provider/src/remote/openai/openai_config.rs`
- `agent/features/runtime/business/chat/slash/switch_model.rs`

## 关联提交

- `111393e fix(provider): 接线 Ollama provider 工厂分支，消除整模块死代码 (refs #85)`
- `679764d docs(bug): #85 修复 commit SHA 回填 (refs #85)`
- `de5a042 fix(runtime): 修复 Ollama 切换模型接线 (refs #85)`
- `ba09882 merge: bug/85-ollama-switch-model — 修复 Ollama 切换模型接线 (refs #85)`