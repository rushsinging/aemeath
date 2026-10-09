//! HfKevTokenizer：kev user-token 语义（special marker 改写、delimiter id、文本编码）。

use crate::adapters::kev_causal_row::{KevDelimiter, KevRowTokenizer};
use crate::adapters::kev_hf_tokenizer::{rewrite_user_text_special_markers, HfKevTokenizer};

/// 最小 WordLevel tokenizer 夹具：覆盖 load、delimiter 查表与 encode 路径。
const MINIMAL_TOKENIZER_JSON: &str = r#"{
  "version": "1.0",
  "truncation": null,
  "padding": null,
  "added_tokens": [
    {"id": 0, "content": "<|fim_prefix|>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 1, "content": "<|fim_middle|>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 2, "content": "<|box_start|>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 3, "content": "<|box_end|>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 4, "content": "<|fim_suffix|>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 5, "content": "<unk>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true}
  ],
  "normalizer": null,
  "pre_tokenizer": {"type": "Whitespace"},
  "post_processor": null,
  "decoder": null,
  "model": {
    "type": "WordLevel",
    "vocab": {
      "<|fim_prefix|>": 0,
      "<|fim_middle|>": 1,
      "<|box_start|>": 2,
      "<|box_end|>": 3,
      "<|fim_suffix|>": 4,
      "<unk>": 5,
      "hello": 6,
      "world": 7
    },
    "unk_token": "<unk>"
  }
}"#;

#[test]
fn rewrite_neutralizes_known_special_markers_only() {
    assert_eq!(
        rewrite_user_text_special_markers("<|fim_prefix|> tail"),
        "<¦fim_prefix¦> tail"
    );
    assert_eq!(
        rewrite_user_text_special_markers("a <|box_end|> b"),
        "a <¦box_end¦> b"
    );
    assert_eq!(
        rewrite_user_text_special_markers("plain text <with|pipe> ok"),
        "plain text <with|pipe> ok",
        "只改写 name 仅含 [A-Za-z0-9_] 的 marker"
    );
    assert_eq!(
        rewrite_user_text_special_markers("<|has space|>"),
        "<|has space|>"
    );
    assert_eq!(
        rewrite_user_text_special_markers("no markers"),
        "no markers"
    );
    assert_eq!(
        rewrite_user_text_special_markers("<|first|>middle<|second|>"),
        "<¦first¦>middle<¦second¦>",
        "多个 marker 全部改写且非重叠"
    );
}

#[test]
fn delimiter_ids_resolve_from_tokenizer_vocab() {
    let tokenizer = HfKevTokenizer::from_json(MINIMAL_TOKENIZER_JSON).expect("夹具加载成功");
    assert_eq!(tokenizer.delimiter_id(KevDelimiter::StateStart).unwrap(), 0);
    assert_eq!(
        tokenizer.delimiter_id(KevDelimiter::QuestionStart).unwrap(),
        1
    );
    assert_eq!(
        tokenizer.delimiter_id(KevDelimiter::OptionStart).unwrap(),
        2
    );
    assert_eq!(tokenizer.delimiter_id(KevDelimiter::OptionEnd).unwrap(), 3);
    assert_eq!(tokenizer.delimiter_id(KevDelimiter::Decide).unwrap(), 4);
}

#[test]
fn tokenize_user_text_encodes_without_special_tokens() {
    let tokenizer = HfKevTokenizer::from_json(MINIMAL_TOKENIZER_JSON).expect("夹具加载成功");
    let token_ids = tokenizer
        .tokenize_user_text("hello world")
        .expect("编码成功");
    assert_eq!(
        token_ids,
        vec![6, 7],
        "WordLevel 分词且不注入 special token"
    );

    let rewritten = tokenizer
        .tokenize_user_text("<|fim_prefix|>")
        .expect("编码成功");
    assert!(
        rewritten.iter().all(|token_id| *token_id >= 5),
        "marker 被改写后 NEVER 命中 special token（0..=4），实际 {rewritten:?}"
    );
}

#[test]
fn from_json_reports_malformed_source() {
    let error = HfKevTokenizer::from_json("not json").expect_err("非法 JSON 必须失败");
    assert!(
        matches!(
            error,
            crate::adapters::kev_hf_tokenizer::TokenizerLoadError::ParseFailed { .. }
        ),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("解析"),
        "错误消息为中文：{error}"
    );
}

/// 真实 kev tokenizer 资产对账：五个 `KevDelimiter::token_text()` MUST 全部命中
/// 本机已安装 / 已缓存的 kev tokenizer 词表（StateStart marker 曾写错，真实资产上
/// `MissingDelimiter`）。本机无资产时打印提示并返回——测试 NEVER 下载、NEVER
/// 硬断言机器相关路径存在。
#[test]
fn real_kev_tokenizer_resolves_all_delimiters_when_installed() {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    let models_dir = share::config::paths::systemone_models_dir();
    if let Ok(entries) = std::fs::read_dir(&models_dir) {
        for entry in entries.flatten() {
            let path = entry
                .path()
                .join(crate::constants::TOKENIZER_JSON_RELATIVE_PATH);
            if path.is_file() {
                candidates.push(path);
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(
            std::path::Path::new(&home)
                .join(".cache/system-one-eval/kev-merged-fp32-backbone/tokenizer.json"),
        );
    }
    let Some(path) = candidates.into_iter().find(|path| path.is_file()) else {
        println!("skip：本机未找到 kev tokenizer 资产，先执行 `aemeath systemone download`");
        return;
    };
    let tokenizer = HfKevTokenizer::from_file(&path)
        .unwrap_or_else(|error| panic!("tokenizer 解析失败：{error}"));
    let mut seen_ids: Vec<i32> = Vec::with_capacity(5);
    for delimiter in [
        KevDelimiter::StateStart,
        KevDelimiter::QuestionStart,
        KevDelimiter::OptionStart,
        KevDelimiter::OptionEnd,
        KevDelimiter::Decide,
    ] {
        let token_id = tokenizer.delimiter_id(delimiter).unwrap_or_else(|error| {
            panic!(
                "真实 tokenizer（{}）缺少 kev 分隔 token：{error}",
                path.display()
            )
        });
        println!("{} -> {token_id}", delimiter.token_text());
        assert!(
            !seen_ids.contains(&token_id),
            "五个分隔 token id 互不相同，重复 {token_id}"
        );
        seen_ids.push(token_id);
    }
}
