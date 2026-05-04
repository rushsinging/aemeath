# Agent Role max_tokens Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow `agents.roles.<role>.max_tokens` to override sub-agent output token limits while treating `0` as inherit/default.

**Architecture:** Add an optional `max_tokens` field to `AgentRoleConfig`. Add runtime max-token setters/getters to `LlmClient`/`LlmProvider` so `CliAgentRunner` can apply role-level overrides after resolving a role and before running the sub-agent. Existing model/provider max-token behavior remains the fallback.

**Tech Stack:** Rust, serde config deserialization, async_trait provider trait, existing aemeath-core/aemeath-llm/aemeath-cli workspace tests.

---

## Files

- Modify: `aemeath-core/src/config/tools.rs` — add `AgentRoleConfig.max_tokens` and config tests.
- Modify: `aemeath-llm/src/provider.rs` — add provider trait methods for runtime max-token override.
- Modify: `aemeath-llm/src/client.rs` — expose `set_max_tokens` / `max_tokens` wrappers.
- Modify: `aemeath-llm/src/providers/openai_compatible/mod.rs` — store max tokens atomically and use current value when building requests.
- Modify: `aemeath-llm/src/providers/openai_compatible/non_stream.rs` — use current max tokens when building non-stream requests.
- Modify: `aemeath-llm/src/providers/ollama/mod.rs` and `aemeath-llm/src/providers/ollama/conversion.rs` — store/use runtime max tokens.
- Modify: `aemeath-llm/src/providers/anthropic.rs` — store/use runtime max tokens.
- Modify: `aemeath-cli/src/agent_runner.rs` — apply role `max_tokens` if `Some(n) && n > 0`; `None` or `0` inherits.

---

### Task 1: Config parsing for role max_tokens

**Files:**
- Modify: `aemeath-core/src/config/tools.rs`

- [ ] **Step 1: Write failing tests at the end of `aemeath-core/src/config/tools.rs`**

Add this block after the `impl Default for AgentsConfig` block:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_role_config_max_tokens_deserialize() {
        let json = r#"{
            "model": "DeepSeek/deepseek-v4-pro",
            "description": "review code",
            "max_tokens": 8192
        }"#;

        let role: AgentRoleConfig = serde_json::from_str(json).unwrap();

        assert_eq!(role.max_tokens, Some(8192));
        assert_eq!(role.model, "DeepSeek/deepseek-v4-pro");
    }

    #[test]
    fn test_agent_role_config_max_tokens_zero_means_inherit() {
        let json = r#"{
            "model": "DeepSeek/deepseek-v4-pro",
            "max_tokens": 0
        }"#;

        let role: AgentRoleConfig = serde_json::from_str(json).unwrap();

        assert_eq!(role.max_tokens, Some(0));
    }

    #[test]
    fn test_agent_role_config_max_tokens_absent_defaults_none() {
        let json = r#"{
            "model": "DeepSeek/deepseek-v4-pro"
        }"#;

        let role: AgentRoleConfig = serde_json::from_str(json).unwrap();

        assert_eq!(role.max_tokens, None);
    }

    #[test]
    fn test_agent_role_config_camel_case_max_tokens_alias() {
        let json = r#"{
            "model": "DeepSeek/deepseek-v4-pro",
            "maxTokens": 4096
        }"#;

        let role: AgentRoleConfig = serde_json::from_str(json).unwrap();

        assert_eq!(role.max_tokens, Some(4096));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p aemeath-core config::tools::tests::test_agent_role_config_max_tokens_deserialize
```

Expected: FAIL with compile error similar to `no field max_tokens on type AgentRoleConfig`.

- [ ] **Step 3: Add field to `AgentRoleConfig`**

In `aemeath-core/src/config/tools.rs`, after the existing `reasoning` field, add:

```rust
    /// Maximum output tokens for sub-agents using this role.
    /// - `None` — inherit from the bound model/provider/default configuration
    /// - `Some(0)` — also inherit, matching model-level max_tokens fallback semantics
    /// - `Some(n)` where n > 0 — override output max tokens for this role
    #[serde(
        default,
        rename = "max_tokens",
        alias = "maxTokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_tokens: Option<u32>,
```

- [ ] **Step 4: Run config tests**

Run:

```bash
cargo test -p aemeath-core config::tools::tests::test_agent_role_config
```

Expected: all four new tests PASS.

- [ ] **Step 5: Commit**

```bash
git add aemeath-core/src/config/tools.rs
git commit -m "feat(config): parse agent role max_tokens"
```

---

### Task 2: Runtime max_tokens interface in LLM client/providers

**Files:**
- Modify: `aemeath-llm/src/provider.rs`
- Modify: `aemeath-llm/src/client.rs`
- Modify: `aemeath-llm/src/providers/openai_compatible/mod.rs`
- Modify: `aemeath-llm/src/providers/openai_compatible/non_stream.rs`
- Modify: `aemeath-llm/src/providers/ollama/mod.rs`
- Modify: `aemeath-llm/src/providers/ollama/conversion.rs`
- Modify: `aemeath-llm/src/providers/anthropic.rs`

- [ ] **Step 1: Add failing tests for OpenAI-compatible runtime override**

In `aemeath-llm/src/providers/openai_compatible/mod.rs`, inside existing `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn openai_provider_set_max_tokens_updates_stream_request_body() {
        let provider = OpenAICompatibleProvider::new(
            OpenAIProviderConfig::from_api_driver(ApiDriverKind::OpenAI, "OpenAI"),
            "test-key".to_string(),
            None,
            Some("gpt-4o".to_string()),
            32000,
            false,
            None,
        );

        provider.set_max_tokens(8192);

        let body = provider.base_request_body(Vec::new(), true);
        assert_eq!(body.get("max_tokens"), Some(&json!(8192)));
    }

    #[test]
    fn openai_provider_set_max_tokens_zero_is_ignored() {
        let provider = OpenAICompatibleProvider::new(
            OpenAIProviderConfig::from_api_driver(ApiDriverKind::OpenAI, "OpenAI"),
            "test-key".to_string(),
            None,
            Some("gpt-4o".to_string()),
            32000,
            false,
            None,
        );

        provider.set_max_tokens(0);

        let body = provider.base_request_body(Vec::new(), true);
        assert_eq!(body.get("max_tokens"), Some(&json!(32000)));
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p aemeath-llm --lib openai_provider_set_max_tokens
```

Expected: FAIL with missing `set_max_tokens` and/or `base_request_body`.

- [ ] **Step 3: Add trait methods in `aemeath-llm/src/provider.rs`**

After `fn is_reasoning(&self) -> bool;`, add:

```rust
    /// Set maximum output tokens at runtime.
    /// Providers should ignore `0` so callers can use it as "inherit/default".
    fn set_max_tokens(&self, _max_tokens: u32) {}

    /// Get current maximum output tokens.
    fn max_tokens(&self) -> u32 {
        0
    }
```

- [ ] **Step 4: Add client wrappers in `aemeath-llm/src/client.rs`**

After `pub fn is_reasoning(&self) -> bool { ... }`, add:

```rust
    pub fn set_max_tokens(&self, max_tokens: u32) {
        self.provider.set_max_tokens(max_tokens);
    }

    pub fn max_tokens(&self) -> u32 {
        self.provider.max_tokens()
    }
```

- [ ] **Step 5: Update OpenAI-compatible provider to use atomic max tokens**

In `aemeath-llm/src/providers/openai_compatible/mod.rs`:

Replace the struct field:

```rust
    max_tokens: u32,
```

with:

```rust
    max_tokens: Arc<std::sync::atomic::AtomicU32>,
```

In `OpenAICompatibleProvider::new`, replace:

```rust
              max_tokens,
```

with:

```rust
              max_tokens: Arc::new(std::sync::atomic::AtomicU32::new(max_tokens)),
```

Add this helper inside `impl OpenAICompatibleProvider`, after `reasoning_handle`:

```rust
    pub(crate) fn current_max_tokens(&self) -> u32 {
        self.max_tokens.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn base_request_body(
        &self,
        messages: Vec<serde_json::Value>,
        stream: bool,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": self.current_max_tokens(),
            "stream": stream,
        });
        if stream {
            body["stream_options"] = serde_json::json!({ "include_usage": true });
        }
        body
    }
```

In `stream_message`, replace the current `serde_json::json!({ ... })` body creation with:

```rust
          let mut request_body = self.base_request_body(openai_messages, true);
```

Inside `impl LlmProvider for OpenAICompatibleProvider`, after `is_reasoning`, add:

```rust
    fn set_max_tokens(&self, max_tokens: u32) {
        if max_tokens > 0 {
            self.max_tokens
                .store(max_tokens, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn max_tokens(&self) -> u32 {
        self.current_max_tokens()
    }
```

- [ ] **Step 6: Update OpenAI-compatible non-stream body**

In `aemeath-llm/src/providers/openai_compatible/non_stream.rs`, replace the request body construction:

```rust
        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": openai_messages,
            "max_tokens": self.max_tokens,
            "stream": false,
        });
```

with:

```rust
        let mut request_body = self.base_request_body(openai_messages, false);
```

- [ ] **Step 7: Update Ollama provider**

In `aemeath-llm/src/providers/ollama/mod.rs`, replace field:

```rust
    pub(crate) max_tokens: u32,
```

with:

```rust
    pub(crate) max_tokens: std::sync::Arc<std::sync::atomic::AtomicU32>,
```

In `OllamaProvider::new`, replace:

```rust
            max_tokens,
```

with:

```rust
            max_tokens: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(max_tokens)),
```

Inside `impl LlmProvider for OllamaProvider`, after `is_reasoning`, add:

```rust
    fn set_max_tokens(&self, max_tokens: u32) {
        if max_tokens > 0 {
            self.max_tokens
                .store(max_tokens, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn max_tokens(&self) -> u32 {
        self.max_tokens.load(std::sync::atomic::Ordering::Relaxed)
    }
```

In `aemeath-llm/src/providers/ollama/conversion.rs`, replace:

```rust
        if self.max_tokens > 0 && self.max_tokens <= 128000 {
            options["num_predict"] = serde_json::json!(self.max_tokens);
        }
```

with:

```rust
        let max_tokens = self.max_tokens.load(std::sync::atomic::Ordering::Relaxed);
        if max_tokens > 0 && max_tokens <= 128000 {
            options["num_predict"] = serde_json::json!(max_tokens);
        }
```

- [ ] **Step 8: Update Anthropic provider**

In `aemeath-llm/src/providers/anthropic.rs`, replace field:

```rust
    max_tokens: u32,
```

with:

```rust
    max_tokens: std::sync::Arc<std::sync::atomic::AtomicU32>,
```

In `AnthropicProvider::new`, replace:

```rust
            max_tokens,
```

with:

```rust
            max_tokens: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(max_tokens)),
```

Replace both occurrences of `self.max_tokens,` used in `CreateMessageRequest::new(...)` calls with:

```rust
              self.max_tokens.load(std::sync::atomic::Ordering::Relaxed),
```

Inside `impl LlmProvider for AnthropicProvider`, after `is_reasoning`, add:

```rust
    fn set_max_tokens(&self, max_tokens: u32) {
        if max_tokens > 0 {
            self.max_tokens
                .store(max_tokens, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn max_tokens(&self) -> u32 {
        self.max_tokens.load(std::sync::atomic::Ordering::Relaxed)
    }
```

- [ ] **Step 9: Run provider tests**

Run:

```bash
cargo test -p aemeath-llm --lib openai_provider_set_max_tokens
cargo test -p aemeath-llm --lib anthropic_request
cargo check -p aemeath-llm
```

Expected: tests PASS and check PASS.

- [ ] **Step 10: Commit**

```bash
git add aemeath-llm/src/provider.rs aemeath-llm/src/client.rs aemeath-llm/src/providers/openai_compatible/mod.rs aemeath-llm/src/providers/openai_compatible/non_stream.rs aemeath-llm/src/providers/ollama/mod.rs aemeath-llm/src/providers/ollama/conversion.rs aemeath-llm/src/providers/anthropic.rs
git commit -m "feat(llm): allow runtime max_tokens override"
```

---

### Task 3: Apply role max_tokens in sub-agent runner

**Files:**
- Modify: `aemeath-cli/src/agent_runner.rs`

- [ ] **Step 1: Write unit helper tests**

In `aemeath-cli/src/agent_runner.rs`, add this helper in `impl CliAgentRunner` after `resolve_role`:

```rust
    fn role_max_tokens_override(role: Option<&AgentRoleConfig>) -> Option<u32> {
        role.and_then(|r| r.max_tokens).filter(|tokens| *tokens > 0)
    }
```

Then add this test module at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_max_tokens_override_uses_positive_value() {
        let role = AgentRoleConfig {
            max_tokens: Some(8192),
            ..Default::default()
        };

        let result = CliAgentRunner::role_max_tokens_override(Some(&role));

        assert_eq!(result, Some(8192));
    }

    #[test]
    fn test_role_max_tokens_override_zero_inherits() {
        let role = AgentRoleConfig {
            max_tokens: Some(0),
            ..Default::default()
        };

        let result = CliAgentRunner::role_max_tokens_override(Some(&role));

        assert_eq!(result, None);
    }

    #[test]
    fn test_role_max_tokens_override_absent_inherits() {
        let role = AgentRoleConfig {
            max_tokens: None,
            ..Default::default()
        };

        let result = CliAgentRunner::role_max_tokens_override(Some(&role));

        assert_eq!(result, None);
    }

    #[test]
    fn test_role_max_tokens_override_no_role_inherits() {
        let result = CliAgentRunner::role_max_tokens_override(None);

        assert_eq!(result, None);
    }
}
```

- [ ] **Step 2: Run test**

Run:

```bash
cargo test -p aemeath-cli agent_runner::tests::test_role_max_tokens_override
```

Expected: PASS after Task 1 has added `AgentRoleConfig.max_tokens`.

- [ ] **Step 3: Apply override in `run_agent`**

In `aemeath-cli/src/agent_runner.rs`, immediately after the code that obtains `client` and before reasoning calculation, add:

```rust
          let role_max_tokens = Self::role_max_tokens_override(role);
          if let Some(max_tokens) = role_max_tokens {
              client.set_max_tokens(max_tokens);
          }
```

Update the existing sub-agent log from:

```rust
              "[SubAgent] reasoning={} (role={:?}, model={:?}, default={})",
              reasoning,
              role_reasoning,
              model_reasoning,
              self.reasoning
```

To:

```rust
              "[SubAgent] reasoning={} max_tokens={:?} (role={:?}, model={:?}, default={})",
              reasoning,
              role_max_tokens,
              role_reasoning,
              model_reasoning,
              self.reasoning
```

- [ ] **Step 4: Run CLI and workspace checks**

Run:

```bash
cargo test -p aemeath-cli agent_runner::tests::test_role_max_tokens_override
cargo check -p aemeath-cli -p aemeath-llm -p aemeath-core
```

Expected: tests PASS and check PASS.

- [ ] **Step 5: Commit**

```bash
git add aemeath-cli/src/agent_runner.rs
git commit -m "feat(agent): apply role max_tokens override"
```

---

### Task 4: Final verification and config example check

**Files:**
- No required source edits unless verification reveals issues.

- [ ] **Step 1: Run formatting**

```bash
cargo fmt
```

Expected: no errors.

- [ ] **Step 2: Run focused tests**

```bash
cargo test -p aemeath-core config::tools::tests::test_agent_role_config
cargo test -p aemeath-llm --lib openai_provider_set_max_tokens
cargo test -p aemeath-cli agent_runner::tests::test_role_max_tokens_override
```

Expected: all PASS.

- [ ] **Step 3: Run broader checks**

```bash
cargo check -p aemeath-core -p aemeath-llm -p aemeath-cli
```

Expected: PASS.

- [ ] **Step 4: Validate example config shape manually**

Use this JSON shape when manually checking global/project config:

```json
{
  "agents": {
    "roles": {
      "coder": {
        "model": "DeepSeek/deepseek-v4-pro",
        "description": "Writes code",
        "reasoning": true,
        "max_tokens": 8192
      }
    }
  }
}
```

Expected: `max_tokens: 8192` applies only for the `coder` role. `max_tokens: 0` or absent inherits from model/provider/default.

- [ ] **Step 5: Inspect diff**

```bash
git diff --stat
```

Expected: changes limited to config, LLM provider runtime max-token support, and agent runner override.

- [ ] **Step 6: Commit any formatting-only changes**

```bash
git add aemeath-core/src/config/tools.rs aemeath-llm/src/provider.rs aemeath-llm/src/client.rs aemeath-llm/src/providers/openai_compatible/mod.rs aemeath-llm/src/providers/openai_compatible/non_stream.rs aemeath-llm/src/providers/ollama/mod.rs aemeath-llm/src/providers/ollama/conversion.rs aemeath-llm/src/providers/anthropic.rs aemeath-cli/src/agent_runner.rs
git commit -m "chore: format agent max_tokens support"
```

Only run this commit if `cargo fmt` changed files not already committed.

---

## Self-review

- Spec coverage: role-level config parsing is covered in Task 1; runtime provider override is covered in Task 2; runner application and `0` inherit behavior are covered in Task 3; verification is covered in Task 4.
- Placeholder scan: no placeholder steps remain; every code change includes exact snippets and commands.
- Type consistency: field name is `max_tokens: Option<u32>` in `AgentRoleConfig`; runtime method names are `set_max_tokens` and `max_tokens` consistently across trait, client, and providers.
