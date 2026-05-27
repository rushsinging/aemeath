# TUI M5 Effect Convergence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 TUI update 后的副作用统一收敛为 `Effect`，由 `EffectExecutor` 执行，执行结果再回到 `Msg`。

**Architecture:** 保留现有 `core::msg::Cmd` 兼容层，新增目标 `effect/` 模块。先实现 `Effect` 类型、executor seam 和 `Cmd ↔ Effect` adapter，再逐步让 coordinator 返回 `Vec<Effect>`。M5 完成后 update/model 不直接执行副作用，副作用统一在 executor 中运行。

**Tech Stack:** Rust 2021、tokio、现有 TUI TEA update、mpsc UiEvent、`cargo test -p cli`、architecture stop hook。

---

## File Structure

- Create: `apps/cli/src/tui/effect/mod.rs` — effect 模块出口。
- Create: `apps/cli/src/tui/effect/effect.rs` — Effect enum。
- Create: `apps/cli/src/tui/effect/result.rs` — EffectResult。
- Create: `apps/cli/src/tui/effect/executor.rs` — EffectExecutor seam。
- Create: `apps/cli/src/tui/effect/legacy_cmd.rs` — 现有 Cmd 兼容 adapter。
- Modify: `apps/cli/src/tui/mod.rs` — 导出 effect。
- Modify: `apps/cli/src/tui/update/mod.rs` 或 `core/update.rs` — 新 coordinator seam 返回 effects。
- Modify: `.agents/hooks/check-tui-tea-purity.sh` or dedicated guard — 禁止 update/model 直接副作用。

## Task 1: Add Effect and EffectResult types

**Files:**
- Create: `apps/cli/src/tui/effect/effect.rs`
- Create: `apps/cli/src/tui/effect/result.rs`
- Create: `apps/cli/src/tui/effect/mod.rs`
- Modify: `apps/cli/src/tui/mod.rs`

- [ ] **Step 1: Write failing effect type test**

Create `apps/cli/src/tui/effect/effect.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_request_render_is_pure_value() {
        let effect = Effect::RequestRender;
        assert_eq!(format!("{effect:?}"), "RequestRender");
    }

    #[test]
    fn test_spawn_agent_chat_carries_chat_id() {
        let effect = Effect::SpawnAgentChat { chat_id: "chat-1".to_string(), prompt: "hello".to_string() };
        assert!(matches!(effect, Effect::SpawnAgentChat { ref chat_id, .. } if chat_id == "chat-1"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::effect::effect::tests
```

Expected: FAIL because effect module is missing.

- [ ] **Step 3: Implement Effect types**

Create `apps/cli/src/tui/effect/effect.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    None,
    RequestRender,
    SpawnAgentChat { chat_id: String, prompt: String },
    SaveSession,
    FetchTaskStatus,
    CopyToClipboard { text: String },
    RunHook { name: String },
    StartTimer { id: String },
    StopTimer { id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_request_render_is_pure_value() {
        let effect = Effect::RequestRender;
        assert_eq!(format!("{effect:?}"), "RequestRender");
    }

    #[test]
    fn test_spawn_agent_chat_carries_chat_id() {
        let effect = Effect::SpawnAgentChat { chat_id: "chat-1".to_string(), prompt: "hello".to_string() };
        assert!(matches!(effect, Effect::SpawnAgentChat { ref chat_id, .. } if chat_id == "chat-1"));
    }
}
```

Create `apps/cli/src/tui/effect/result.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectResult {
    RenderRequested,
    AgentChatSpawned { chat_id: String },
    SessionSaved,
    TaskStatusFetched,
    ClipboardCopied,
    HookFinished { name: String, success: bool },
    TimerStarted { id: String },
    TimerStopped { id: String },
    Failed { message: String },
}
```

Create `apps/cli/src/tui/effect/mod.rs`:

```rust
pub mod effect;
pub mod result;

pub use effect::Effect;
pub use result::EffectResult;
```

Modify `apps/cli/src/tui/mod.rs`:

```rust
pub mod effect;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::effect::effect::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/mod.rs apps/cli/src/tui/effect
git commit -m "feat: add TUI effect types"
```

## Task 2: Add EffectExecutor seam

**Files:**
- Create: `apps/cli/src/tui/effect/executor.rs`
- Modify: `apps/cli/src/tui/effect/mod.rs`

- [ ] **Step 1: Write failing executor test**

Create `apps/cli/src/tui/effect/executor.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::effect::{Effect, EffectResult};

    #[tokio::test]
    async fn test_executor_handles_request_render_without_io() {
        let mut executor = EffectExecutor::default();
        let result = executor.execute(Effect::RequestRender).await;
        assert_eq!(result, EffectResult::RenderRequested);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::effect::executor::tests::test_executor_handles_request_render_without_io
```

Expected: FAIL because executor is missing.

- [ ] **Step 3: Implement minimal executor**

Create `apps/cli/src/tui/effect/executor.rs`:

```rust
use super::{Effect, EffectResult};

#[derive(Default)]
pub struct EffectExecutor;

impl EffectExecutor {
    pub async fn execute(&mut self, effect: Effect) -> EffectResult {
        match effect {
            Effect::None => EffectResult::RenderRequested,
            Effect::RequestRender => EffectResult::RenderRequested,
            Effect::SpawnAgentChat { chat_id, .. } => EffectResult::AgentChatSpawned { chat_id },
            Effect::SaveSession => EffectResult::SessionSaved,
            Effect::FetchTaskStatus => EffectResult::TaskStatusFetched,
            Effect::CopyToClipboard { .. } => EffectResult::ClipboardCopied,
            Effect::RunHook { name } => EffectResult::HookFinished { name, success: true },
            Effect::StartTimer { id } => EffectResult::TimerStarted { id },
            Effect::StopTimer { id } => EffectResult::TimerStopped { id },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::effect::{Effect, EffectResult};

    #[tokio::test]
    async fn test_executor_handles_request_render_without_io() {
        let mut executor = EffectExecutor::default();
        let result = executor.execute(Effect::RequestRender).await;
        assert_eq!(result, EffectResult::RenderRequested);
    }
}
```

Modify `apps/cli/src/tui/effect/mod.rs`:

```rust
pub mod effect;
pub mod executor;
pub mod result;

pub use effect::Effect;
pub use executor::EffectExecutor;
pub use result::EffectResult;
```

- [ ] **Step 4: Run test**

```bash
cargo test -p cli tui::effect::executor::tests::test_executor_handles_request_render_without_io
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/effect
git commit -m "feat: add TUI effect executor seam"
```

## Task 3: Add legacy Cmd adapter

**Files:**
- Create: `apps/cli/src/tui/effect/legacy_cmd.rs`
- Modify: `apps/cli/src/tui/effect/mod.rs`

- [ ] **Step 1: Inspect existing Cmd variants**

Read `apps/cli/src/tui/core/msg.rs` and record the current `Cmd` variants in implementation notes.

- [ ] **Step 2: Write failing adapter test**

Create `apps/cli/src/tui/effect/legacy_cmd.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::tui::core::msg::Cmd;
    use crate::tui::effect::Effect;

    use super::effect_from_legacy_cmd;

    #[test]
    fn test_none_cmd_maps_to_none_effect() {
        assert_eq!(effect_from_legacy_cmd(Cmd::None), Effect::None);
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

```bash
cargo test -p cli tui::effect::legacy_cmd::tests::test_none_cmd_maps_to_none_effect
```

Expected: FAIL because adapter is missing or incomplete.

- [ ] **Step 4: Implement minimal adapter**

Create `apps/cli/src/tui/effect/legacy_cmd.rs`:

```rust
use crate::tui::core::msg::Cmd;

use super::Effect;

pub fn effect_from_legacy_cmd(cmd: Cmd) -> Effect {
    match cmd {
        Cmd::None => Effect::None,
        _ => Effect::RequestRender,
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::core::msg::Cmd;
    use crate::tui::effect::Effect;

    use super::effect_from_legacy_cmd;

    #[test]
    fn test_none_cmd_maps_to_none_effect() {
        assert_eq!(effect_from_legacy_cmd(Cmd::None), Effect::None);
    }
}
```

Modify `apps/cli/src/tui/effect/mod.rs`:

```rust
pub mod effect;
pub mod executor;
pub mod legacy_cmd;
pub mod result;

pub use effect::Effect;
pub use executor::EffectExecutor;
pub use legacy_cmd::effect_from_legacy_cmd;
pub use result::EffectResult;
```

- [ ] **Step 5: Run test and commit**

```bash
cargo test -p cli tui::effect::legacy_cmd::tests::test_none_cmd_maps_to_none_effect
cargo check -p cli
git add apps/cli/src/tui/effect
git commit -m "feat: add legacy cmd to effect adapter"
```

Expected: tests/check PASS, commit succeeds.

## Task 4: Add coordinator effect seam

**Files:**
- Create: `apps/cli/src/tui/update/coordinator.rs`
- Modify: `apps/cli/src/tui/update/mod.rs`

- [ ] **Step 1: Write failing coordinator test**

Create `apps/cli/src/tui/update/coordinator.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::tui::effect::Effect;
    use crate::tui::model::input::{InputChange, InputSubmission};

    use super::effects_for_input_change;

    #[test]
    fn test_submitted_input_requests_render() {
        let effects = effects_for_input_change(&InputChange::Submitted { submission: InputSubmission { text: "hello".to_string(), attachments: Vec::new() } });
        assert!(effects.contains(&Effect::RequestRender));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::update::coordinator::tests::test_submitted_input_requests_render
```

Expected: FAIL because coordinator seam is missing.

- [ ] **Step 3: Implement coordinator seam**

Create `apps/cli/src/tui/update/coordinator.rs`:

```rust
use crate::tui::effect::Effect;
use crate::tui::model::input::InputChange;

pub fn effects_for_input_change(change: &InputChange) -> Vec<Effect> {
    match change {
        InputChange::TextChanged { .. } | InputChange::CursorMoved { .. } | InputChange::Submitted { .. } | InputChange::Cleared => vec![Effect::RequestRender],
    }
}
```

Modify `apps/cli/src/tui/update/mod.rs`:

```rust
pub mod coordinator;
pub mod input_mapper;
```

- [ ] **Step 4: Run test**

```bash
cargo test -p cli tui::update::coordinator::tests::test_submitted_input_requests_render
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/update
git commit -m "feat: add TUI update effect seam"
```

## Task 5: Add Effect boundary architecture guard

**Files:**
- Create: `.agents/hooks/check-tui-effect-boundary.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`

- [ ] **Step 1: Create guard**

Create `.agents/hooks/check-tui-effect-boundary.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(pwd)}"
cd "$ROOT"

fail=0

for dir in apps/cli/src/tui/model apps/cli/src/tui/update; do
  [ -d "$dir" ] || continue
  if grep -R "tokio::spawn\|std::thread::spawn\|Command::new\|\.await\|mpsc::Sender" "$dir" -n --include='*.rs'; then
    echo "[architecture] model/update must return Effect instead of executing side effects" >&2
    fail=1
  fi
done

exit "$fail"
```

- [ ] **Step 2: Wire guard**

```bash
chmod +x .agents/hooks/check-tui-effect-boundary.sh
```

Add to `.agents/hooks/check-architecture-guards.sh` near other TUI guards:

```bash
run_guard "check-tui-effect-boundary.sh"
```

Use the existing helper name from the script if it differs.

- [ ] **Step 3: Run guard**

```bash
.agents/hooks/check-tui-effect-boundary.sh
.agents/hooks/check-architecture-guards.sh
```

Expected: PASS.

- [ ] **Step 4: Run full validation**

```bash
cargo test -p cli tui::effect tui::update::coordinator
cargo check -p cli
.agents/hooks/check-architecture-guards.sh
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add .agents/hooks/check-tui-effect-boundary.sh .agents/hooks/check-architecture-guards.sh
git commit -m "chore: guard TUI effect boundary"
```

## Final verification

Run:

```bash
cargo test -p cli
cargo check -p cli
.agents/hooks/check-architecture-guards.sh
```

Expected: all PASS.

M5 is complete when new code can describe side effects with `Effect`, execute them through `EffectExecutor`, and guards prevent model/update from directly running side effects.
