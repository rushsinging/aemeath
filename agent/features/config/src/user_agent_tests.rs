//! `agent/features/config/src/user_agent.rs` 的契约测试。
//!
//! 覆盖目标：
//! - 四级优先级严格：Provider 专属 → Catalog 官方 SDK 默认 → 全局配置 → 全局默认；
//! - 空白字符串等同未配置并继续回退；
//! - 系统版本不可用时降级为 `Aemeath/<version> cli <os>/<arch>`；
//! - 动态片段必须 HeaderValue 安全（不含控制字符）；
//! - UA 不得泄漏 API Key / session id / 项目路径；
//! - 全局默认 UA 严格遵守 §5.2 格式。

use crate::catalog::{find_by_driver, OfficialSdkUserAgent};
use crate::ports::SystemInformation;
use crate::user_agent::{
    build_global_default_user_agent, resolve_provider_user_agent, resolve_provider_user_agent_str,
    ProviderUserAgentInputs,
};
use http::HeaderValue;
use share::config::models::ProviderModelsConfig;

fn system(os: &str, arch: &str, version: Option<&str>) -> SystemInformation {
    SystemInformation {
        os_name: os.to_string(),
        os_version: version.map(str::to_string),
        arch: arch.to_string(),
    }
}

fn provider_config(user_agent: Option<&str>) -> ProviderModelsConfig {
    ProviderModelsConfig {
        base_url: "https://example.test".to_string(),
        api_key: String::new(),
        driver: "anthropic".to_string(),
        models: vec![],
        user_agent: user_agent.map(str::to_string),
    }
}

#[test]
fn provider_specific_user_agent_wins_even_when_global_is_set() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: Some("custom-cli/1.0"),
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("global/2.0"),
        system: system("macos", "aarch64", Some("15.5")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "custom-cli/1.0");
}

#[test]
fn catalog_official_sdk_user_agent_used_when_provider_missing() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: Some(HeaderValue::from_static("anthropic-sdk/0.1")),
        global_user_agent: Some("global/2.0"),
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "anthropic-sdk/0.1");
}

#[test]
fn global_user_agent_used_when_provider_and_catalog_missing() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("global/2.0"),
        system: system("macos", "aarch64", Some("15.5")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "global/2.0");
}

#[test]
fn global_default_used_when_every_other_source_is_missing() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: None,
        global_user_agent: None,
        system: system("macos", "aarch64", Some("15.5")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "Aemeath/0.1.0 cli macos/15.5/aarch64");
}

#[test]
fn blank_provider_user_agent_falls_through_to_catalog() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: Some("   \t\n  "),
        catalog_official_sdk_user_agent: Some(HeaderValue::from_static("anthropic-sdk/0.1")),
        global_user_agent: None,
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "anthropic-sdk/0.1");
}

#[test]
fn blank_provider_user_agent_falls_through_to_global_config() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: Some("   \t\n  "),
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("global/2.0"),
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "global/2.0");
}

#[test]
fn blank_provider_user_agent_falls_through_to_global_default() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: Some("   "),
        catalog_official_sdk_user_agent: None,
        global_user_agent: None,
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "Aemeath/0.1.0 cli linux/6.1.0/x86_64");
}

#[test]
fn blank_global_user_agent_falls_through_to_global_default() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("   \t\n  "),
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "Aemeath/0.1.0 cli linux/6.1.0/x86_64");
}

#[test]
fn missing_os_version_degrades_global_default_to_two_segment_form() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: None,
        global_user_agent: None,
        system: system("linux", "x86_64", None),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "Aemeath/0.1.0 cli linux/x86_64");
}

#[test]
fn blank_os_version_degrades_global_default_to_two_segment_form() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: None,
        global_user_agent: None,
        system: system("linux", "x86_64", Some("   ")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "Aemeath/0.1.0 cli linux/x86_64");
}

#[test]
fn invalid_provider_user_agent_falls_through_to_next_source() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: Some("bad\rinjection"),
        catalog_official_sdk_user_agent: Some(HeaderValue::from_static("anthropic-sdk/0.1")),
        global_user_agent: Some("global/2.0"),
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(
        resolved, "anthropic-sdk/0.1",
        "含控制字符的 provider UA 必须被拒绝并回退到 Catalog"
    );
}

#[test]
fn invalid_global_user_agent_falls_through_to_global_default() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("bad\nvalue"),
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(resolved, "Aemeath/0.1.0 cli linux/6.1.0/x86_64");
}

#[test]
fn sanitize_segment_replaces_non_ascii_with_dash() {
    // 非 ASCII（含中日韩）片段不得保留。HeaderValue 要求可见 ASCII token，扩展字符
    // 会让 `from_str` 后续失败；resolver 必须在 fragment 阶段就替换为 `-`。
    let system = SystemInformation {
        os_name: "系统".to_string(),
        os_version: Some("版-本".to_string()),
        arch: "アーキ".to_string(),
    };
    let ua = build_global_default_user_agent(&system, "1.0.0");
    let resolved = ua.to_str().expect("UA 必须可转 ASCII");
    assert!(
        !resolved.contains('系') && !resolved.contains('版') && !resolved.contains('ア'),
        "中日韩片段必须被替换，得到 {resolved}"
    );
    assert!(
        resolved.contains("---/---/----") || resolved.matches('-').count() >= 4,
        "每个非 ASCII 字符都必须替换为 `-`，得到 {resolved}"
    );
    assert!(
        HeaderValue::from_str(resolved).is_ok(),
        "替换后的 UA 必须仍是合法 HeaderValue，得到 {resolved}"
    );
}

#[test]
fn sanitize_segment_replaces_control_characters_with_dash() {
    // 平台信息里的 NUL/CR/LF/换页/Backspace 视为控制字符，全部替换为 `-`。
    let system = SystemInformation {
        os_name: "mac\ros".to_string(),
        os_version: Some("ver\nsion".to_string()),
        arch: "arch\u{0000}end".to_string(),
    };
    let ua = build_global_default_user_agent(&system, "0.1.0");
    let resolved = ua.to_str().expect("UA 必须可转 ASCII");
    assert!(!resolved.contains('\n'));
    assert!(!resolved.contains('\r'));
    assert!(!resolved.contains('\0'));
    assert!(
        HeaderValue::from_str(resolved).is_ok(),
        "控制字符必须已替换，得到 {resolved:?}"
    );
}

#[test]
fn sanitize_segment_keeps_ascii_whitespace_intact() {
    // 空白（空格 / TAB）是合法 ASCII 可见字符，与控制字符不同；`sanitize_segment`
    // 必须保留 ASCII 空白而仅替换 ASCII 控制字符（< 0x20 或 0x7F）。
    let system = SystemInformation {
        os_name: "mac os".to_string(),
        os_version: None,
        arch: "arm 64".to_string(),
    };
    let ua = build_global_default_user_agent(&system, "0.1.0");
    let resolved = ua.to_str().expect("UA 必须可转 ASCII");
    assert!(
        resolved.contains("mac os/arm 64"),
        "ASCII 空白必须保留，得到 {resolved}"
    );
}

#[test]
fn build_global_default_user_agent_falls_back_when_segment_is_entirely_non_ascii() {
    // 整个片段都是非 ASCII，必须降级到占位符，而不是让 `HeaderValue::from_str` 失败。
    let system = SystemInformation {
        os_name: "中".to_string(),
        os_version: None,
        arch: "文".to_string(),
    };
    let ua = build_global_default_user_agent(&system, "0.1.0");
    let resolved = ua.to_str().expect("UA 必须可转 ASCII");
    assert_eq!(resolved, "Aemeath/0.1.0 cli unknown-os/unknown-arch");
}

#[test]
fn parse_header_value_rejects_blank_and_nul_in_provider_user_agent() {
    // resolve_provider_user_agent 内部对 provider UA 用 parse_header_value 兜底：
    // 空白 / NUL / 含控制字符 / 非 ASCII 字符串必须全部返回 None，让回退继续。
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: Some("\u{0000}nul-prefix"),
        catalog_official_sdk_user_agent: Some(HeaderValue::from_static("anthropic-sdk/0.1")),
        global_user_agent: Some("global/2.0"),
        system: system("macos", "aarch64", Some("15.5")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(
        resolved, "anthropic-sdk/0.1",
        "含 NUL 的 provider UA 必须拒绝并回退到 Catalog"
    );
}

#[test]
fn parse_header_value_rejects_non_ascii_provider_user_agent() {
    // `HeaderValue::from_str` 会接受非 ASCII 字节但 `to_str` 失败；
    // resolver 必须主动拒绝这类输入并回退。
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: Some("自定义cli/1.0"),
        catalog_official_sdk_user_agent: Some(HeaderValue::from_static("anthropic-sdk/0.1")),
        global_user_agent: Some("global/2.0"),
        system: system("macos", "aarch64", Some("15.5")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(
        resolved, "anthropic-sdk/0.1",
        "含非 ASCII 的 provider UA 必须拒绝并回退到 Catalog（不能让 to_str 失败）"
    );
}

#[test]
fn parse_header_value_rejects_nul_in_global_user_agent() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: Some("\u{0000}"),
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("bad\0global"),
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(
        resolved, "Aemeath/0.1.0 cli linux/6.1.0/x86_64",
        "含 NUL 的 global UA 必须拒绝并回退到全局默认"
    );
}

#[test]
fn resolver_skips_to_global_default_when_only_global_ua_is_non_ascii() {
    // 全局配置包含非 ASCII。resolver 必须跳过 global 一级并落到默认。
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("中文代理/1.0"),
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    assert_eq!(
        resolved, "Aemeath/0.1.0 cli linux/6.1.0/x86_64",
        "含非 ASCII 的 global UA 必须拒绝并回退到全局默认"
    );
}

#[test]
fn dynamic_segments_strip_control_characters_but_preserve_whitespace() {
    // 系统信息片段中的控制字符（CR / LF / NUL / 换页）必须被替换为 `-`；
    // ASCII 空白（空格 / TAB）必须保留，由 HeaderValue 校验保证管道可达。
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: None,
        global_user_agent: None,
        system: system("mac os\n15.5", "arm 64", None),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    // 控制字符必须被剥离。
    assert!(!resolved.contains('\n'));
    assert!(!resolved.contains('\r'));
    // 必须保持 `Aemeath/<version> cli <os>/<arch>` 框架。
    assert!(resolved.starts_with("Aemeath/0.1.0 cli "));
}

#[test]
fn global_default_user_agent_uses_compiled_version_when_available() {
    let ua = build_global_default_user_agent(&system("macos", "aarch64", Some("15.5")), "0.2.3");
    assert_eq!(ua.to_str().unwrap(), "Aemeath/0.2.3 cli macos/15.5/aarch64");
}

#[test]
fn global_default_user_agent_handles_missing_version_and_blank_inputs() {
    let system = system("linux", "x86_64", None);
    let ua = build_global_default_user_agent(&system, "0.0.0");
    assert_eq!(ua.to_str().unwrap(), "Aemeath/0.0.0 cli linux/x86_64");
}

#[test]
fn resolve_provider_user_agent_returns_header_value_safe_header() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: Some("custom-cli/1.0"),
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("global/2.0"),
        system: system("macos", "aarch64", Some("15.5")),
        version: "0.1.0",
    };
    let header = resolve_provider_user_agent(inputs);
    assert_eq!(header.to_str().unwrap(), "custom-cli/1.0");
    // 整个 header 必须可被 http 层解析。
    assert!(HeaderValue::from_str(header.to_str().unwrap()).is_ok());
}

#[test]
fn provider_user_agent_integration_with_normalized_user_agent() {
    // 集成测试：从 `ProviderModelsConfig::normalized_user_agent()` 拿到空时，
    // resolver 必须跳过 Provider 专属一级，回到 Catalog / 全局 / 全局默认。
    let config = provider_config(Some("   \t  "));
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: config.normalized_user_agent(),
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("global/2.0"),
        system: system("macos", "aarch64", Some("15.5")),
        version: "0.1.0",
    };
    assert_eq!(
        resolve_provider_user_agent_str(inputs),
        "global/2.0",
        "ProviderModelsConfig 空白 UA 必须归一为 None 并回退到全局"
    );
}

#[test]
fn provider_user_agent_integration_with_non_blank_value() {
    let config = provider_config(Some("custom-cli/1.0"));
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: config.normalized_user_agent(),
        catalog_official_sdk_user_agent: None,
        global_user_agent: Some("global/2.0"),
        system: system("macos", "aarch64", Some("15.5")),
        version: "0.1.0",
    };
    assert_eq!(
        resolve_provider_user_agent_str(inputs),
        "custom-cli/1.0",
        "ProviderModelsConfig 非空白 UA 必须作为最高优先级生效"
    );
}

#[test]
fn ua_resolver_never_leaks_api_key_or_path_in_resolved_value() {
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: None,
        global_user_agent: None,
        system: system("macos", "aarch64", Some("15.5")),
        version: "0.1.0",
    };
    let resolved = resolve_provider_user_agent_str(inputs);
    for forbidden in ["key=", "Bearer ", "/Users/", "/home/", "session=", "sid="] {
        assert!(
            !resolved.contains(forbidden),
            "UA 不得泄漏敏感片段 {forbidden:?}，得到 {resolved}"
        );
    }
}

#[test]
fn catalog_lookup_for_anthropic_returns_no_official_sdk_user_agent_by_default() {
    // 显式断言当前所有 driver 都没有可靠官方 SDK UA 证据；
    // 未来官方 SDK UA 接入 Catalog 时，必须把证据元数据写入 OfficialSdkUserAgent。
    let entry = find_by_driver("anthropic").expect("anthropic 必须存在");
    assert!(
        entry.official_sdk_user_agent.is_none(),
        "anthropic 暂无可靠官方 SDK UA 证据，必须为 None"
    );

    // 顺手断言其余 driver 都同样为 None（避免单点遗漏）。
    for driver in [
        "openai",
        "zhipu",
        "litellm",
        "volcengine",
        "minimax",
        "mimo",
        "deepseek",
        "agnes",
        "ollama",
    ] {
        let entry = find_by_driver(driver).unwrap_or_else(|| panic!("Catalog 必含 {driver}"));
        assert!(
            entry.official_sdk_user_agent.is_none(),
            "{driver} 暂无可靠官方 SDK UA 证据，必须为 None"
        );
    }
}

#[test]
fn official_sdk_user_agent_with_valid_value_is_usable_in_resolver() {
    // 单独构造带 evidence 的值对象：模拟未来 Catalog 引入官方 SDK UA 时的形态。
    // resolver 路径不再依赖 Catalog 自身的数据，而是接收构造好的 HeaderValue。
    let official = OfficialSdkUserAgent {
        sdk_name: "anthropic-sdk",
        sdk_version: "0.1.0",
        value: HeaderValue::from_static("anthropic-sdk/0.1.0"),
        evidence_url: "https://docs.example.com/ua",
        verified_at: chrono::NaiveDate::from_ymd_opt(2026, 7, 21).unwrap(),
    };
    let header_value = official.value.clone();
    let inputs = ProviderUserAgentInputs {
        provider_user_agent: None,
        catalog_official_sdk_user_agent: Some(header_value),
        global_user_agent: None,
        system: system("linux", "x86_64", Some("6.1.0")),
        version: "0.1.0",
    };
    assert_eq!(
        resolve_provider_user_agent_str(inputs),
        "anthropic-sdk/0.1.0"
    );
}
