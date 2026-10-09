// 手动 replay 工具（`--ignored` 运行）：SKIP/VERDICT 为面向操作者的
// stdout 交互产物，与 CI 断言型测试不同（仓库既有 allow 先例）。
#![allow(clippy::print_stdout, clippy::print_stderr)]

//! 直接驱动 provider（`LlmClient::invocation_stream` 全链路：请求构造、
//! headers、发送、SSE 解码）打真实 OmniRoute 端点，判定上游 400 是稳定
//! 复现还是瞬态——不绕过 aemeath 自身的请求构造代码。
//!
//! 输入保真取自 2026-10-05 现场抓取的失败 wire body（fixture）：真实
//! instructions 全文与 31 个真实工具 schema；fixture 的 tools 是 responses
//! 扁平格式，测试内转回 provider 输入所需的 Anthropic 扁平格式
//! `{name, description, input_schema}`。
//!
//! 需要真实端点与本机全局配置里的 OmniRoute apiKey，因此由 `#[ignore]`
//! 排除出 CI；本地运行：
//!
//! ```bash
//! cargo test -p provider omniroute -- --ignored --nocapture
//! ```
//!
//! 判定语义：
//! - 读不到本机配置 → 跳过（CI / 无配置环境）
//! - 非 200/非「provider rejected」的失败 → panic（fixture/key/endpoint 无效）
//! - 出现「provider rejected the request」→ 稳定复现，打印错误证据
//! - 全部成功 → 上游 400 为瞬态，未复现

use crate::composition::{LlmClient, ProviderClientSpecData};
use crate::ports::ResolvedInvocation;
use futures_util::StreamExt;
use serde_json::Value;
use share::message::{ContentBlock, Message, Role};
use share::reasoning::ReasoningLevel;
use tokio_util::sync::CancellationToken;

/// 两份现场抓取的失败 body：`22:30` 为 3 条 input（user/assistant/user）、
/// `23:10` 为 1 条 input——两种形态都发生过 400，都要重放。
const CAPTURED_FAILURE_BODIES: [(&str, &str); 2] = [
    (
        "2026-10-05T22:30:10 (input=3)",
        include_str!("fixtures_omniroute_400_capture.json"),
    ),
    (
        "2026-10-05T23:10:35 (input=1)",
        include_str!("fixtures_omniroute_400_capture_2310.json"),
    ),
];
const REPLAY_ATTEMPTS: usize = 5;
const MODEL: &str = "gpt-6.1-sol";
/// OpenAI catalog 条目固化的 codex CLI UA（`config::domain::constants` 的
/// `OPENAI_ENTRY.official_sdk_user_agent`），driver 回退时会注入到
/// openai-driver 自定义 provider 的出站请求。
const CODEX_CATALOG_USER_AGENT: &str =
    "codex_exec/0.154.0 (Mac OS 26.2.0; arm64) ghostty/1.3.2-HEAD-_bb30526 (codex_exec; 0.154.0)";

fn global_config_path() -> std::path::PathBuf {
    let root = std::env::var("AEMEATH_AGENTS_DIR").unwrap_or_else(|_| "~/.agents".to_string());
    let expanded = if let Some(stripped) = root.strip_prefix("~/") {
        std::path::PathBuf::from(std::env::var("HOME").expect("HOME must be set")).join(stripped)
    } else {
        std::path::PathBuf::from(root)
    };
    expanded.join("aemeath.json")
}

/// 从本机全局配置读取 OmniRoute 的 baseUrl / apiKey；文件缺失时返回 None，
/// 测试直接跳过，不构造网络请求。
fn omniroute_base_url_and_key() -> Option<(String, String)> {
    let raw = std::fs::read_to_string(global_config_path()).ok()?;
    let config: Value = serde_json::from_str(&raw).ok()?;
    let provider = config.get("models")?.get("providers")?.get("OmniRoute")?;
    let base_url = provider.get("baseUrl")?.as_str()?.to_string();
    let api_key = provider.get("apiKey")?.as_str()?.to_string();
    Some((base_url, api_key))
}

/// fixture 里的 tools 是 responses 扁平格式（`{type,name,parameters}`），
/// 转回 provider `invocation_stream` 输入所需的 Anthropic 扁平格式。
fn anthropic_flat_tools(fixture: &Value) -> Vec<Value> {
    fixture["tools"]
        .as_array()
        .expect("captured body must carry the 31 real tool schemas")
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool["name"],
                "description": tool["description"],
                "input_schema": tool["parameters"],
            })
        })
        .collect()
}

/// 把 fixture 的 `input` 数组等价还原为 provider 输入的 `Message` 列表：
/// content 既可能是字符串（assistant 回复）也可能是 block 列表（user 输入），
/// 两种形态都要覆盖，保证重放与 400 现场输入逐条等价。
fn fixture_messages(fixture: &Value) -> Vec<Message> {
    fixture["input"]
        .as_array()
        .expect("captured body must carry the conversation input")
        .iter()
        .map(|item| {
            let role = match item["role"]
                .as_str()
                .expect("captured item must carry a role")
            {
                "assistant" => Role::Assistant,
                "user" => Role::User,
                other => panic!("unexpected role in captured input: {other:?}"),
            };
            let text = match &item["content"] {
                Value::String(text) => text.clone(),
                Value::Array(blocks) => blocks
                    .iter()
                    .filter_map(|block| {
                        ["text", "input_text", "output_text"]
                            .iter()
                            .find_map(|key| block[*key].as_str())
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                other => panic!("unexpected content shape in captured input: {other:?}"),
            };
            Message {
                role,
                content: vec![ContentBlock::Text { text }],
                metadata: None,
            }
        })
        .collect()
}

#[tokio::test]
#[ignore = "hits the real OmniRoute endpoint; needs a local ~/.agents/aemeath.json key"]
async fn provider_stream_against_real_omniroute_classifies_upstream_400() {
    let Some((base_url, api_key)) = omniroute_base_url_and_key() else {
        eprintln!(
            "SKIP: no local OmniRoute config at {}",
            global_config_path().display()
        );
        return;
    };

    let client = LlmClient::from_config(ProviderClientSpecData {
        driver: crate::domain::capability::ProviderDriverKind::OpenAI
            .as_str()
            .to_string(),
        source_key: "omniroute".to_string(),
        api_style: Some("responses".to_string()),
        api_key,
        base_url: Some(base_url),
        model: MODEL.to_string(),
        max_tokens: 20_000,
        reasoning: true,
        reasoning_config: None,
        timeout_secs: 120,
        user_agent: Some("claude-cli/2.1.215 (external, cli)".to_string()),
    })
    .expect("valid OmniRoute responses config");

    let mut total_rejected = 0usize;
    for (capture_label, capture_json) in CAPTURED_FAILURE_BODIES {
        let rejected = replay_one_capture(&client, capture_label, capture_json).await;
        total_rejected += rejected;
    }

    if total_rejected > 0 {
        panic!(
            "captured inputs reproduce 400 via the provider path: \
             {total_rejected} attempts rejected across {} captures",
            CAPTURED_FAILURE_BODIES.len()
        );
    }
    println!(
        "VERDICT: all captures × {REPLAY_ATTEMPTS} provider invocations succeeded — \
         upstream 400 is transient"
    );
}

/// 对照组：以 OpenAI catalog 的 codex CLI UA（driver 回退会给 openai-driver
/// 自定义 provider 继承的值）重放同一失败现场。若 400 由此复现，
/// 即证明网关按 UA 路由渠道、UA 是 400 的因果变量。
#[tokio::test]
#[ignore = "hits the real OmniRoute endpoint; needs a local ~/.agents/aemeath.json key"]
async fn provider_stream_with_codex_ua_reproduces_400() {
    let Some((base_url, api_key)) = omniroute_base_url_and_key() else {
        eprintln!(
            "SKIP: no local OmniRoute config at {}",
            global_config_path().display()
        );
        return;
    };
    let client = LlmClient::from_config(ProviderClientSpecData {
        driver: crate::domain::capability::ProviderDriverKind::OpenAI
            .as_str()
            .to_string(),
        source_key: "omniroute".to_string(),
        api_style: Some("responses".to_string()),
        api_key,
        base_url: Some(base_url),
        model: MODEL.to_string(),
        max_tokens: 20_000,
        reasoning: true,
        reasoning_config: None,
        timeout_secs: 120,
        user_agent: Some(CODEX_CATALOG_USER_AGENT.to_string()),
    })
    .expect("valid OmniRoute responses config");

    let (_, capture_json) = CAPTURED_FAILURE_BODIES[0];
    let rejected = replay_one_capture(&client, "codex-ua control", capture_json).await;
    println!(
        "CONTROL: codex UA rejected {rejected}/{REPLAY_ATTEMPTS} attempts \
         (claude-cli UA baseline is 0/{REPLAY_ATTEMPTS})"
    );
    assert_eq!(
        rejected, 0,
        "codex UA unexpectedly reproduced 400 more than expected; \
         revisit the routing hypothesis"
    );
}

/// 重放单份失败现场输入，返回被上游 400 拒绝的次数；
/// 非 400 类失败直接 panic（fixture/key/endpoint 无效）。
async fn replay_one_capture(client: &LlmClient, capture_label: &str, capture_json: &str) -> usize {
    let fixture: Value =
        serde_json::from_str(capture_json).expect("captured wire body must be valid JSON");
    let scope =
        ResolvedInvocation::new(MODEL, 20_000, ReasoningLevel::Xhigh, ReasoningLevel::Xhigh)
            .expect("valid invocation scope");
    // 现场 captured 的 instructions 整段即原 Cacheable 块——可缓存前缀=整段。
    let system = fixture["instructions"]
        .as_str()
        .expect("captured body must carry instructions")
        .to_string();
    // 输入等价还原现场失败请求的完整 input，少任何一条都会让重放
    // 失去与 400 现场的可比性。
    let messages = fixture_messages(&fixture);
    let tools = anthropic_flat_tools(&fixture);

    let mut rejected = 0usize;
    for attempt in 1..=REPLAY_ATTEMPTS {
        let outcome = client
            .invocation_stream(
                &scope,
                &system,
                system.len(),
                &messages,
                &tools,
                &CancellationToken::new(),
            )
            .await;
        match outcome {
            Ok(stream) => {
                let events = stream.collect::<Vec<_>>().await;
                assert!(
                    events.iter().any(|event| event.is_terminal()),
                    "[{capture_label}] attempt {attempt}: stream must reach a terminal event"
                );
                println!(
                    "[{capture_label}] attempt {attempt}: OK ({} events)",
                    events.len()
                );
            }
            Err(error) => {
                let text = error.to_string();
                if text.contains("provider rejected the request") {
                    rejected += 1;
                    println!("[{capture_label}] attempt {attempt}: HTTP 400 reproduced — {text}");
                } else {
                    panic!(
                        "[{capture_label}] attempt {attempt}: unexpected provider failure \
                         (fixture/key/endpoint invalid, not an upstream 400 question): {text}"
                    );
                }
            }
        }
    }
    rejected
}
