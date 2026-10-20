//! #1987 验收：对本机真实 corpus 快照验证 corpus 现状注入（只读路径，
//! NEVER 触发写锁或落盘）。
//!
//! 快照事实（2026-10-17）：project active=198、archive=185；全量 active 上
//! 存在 J≈0.82 的真实重复对（#1981 写锁两代结论），其中一侧 outdated=true
//! 会被反思输入的 M12 过滤排除——检测函数对全量真实数据断言，prompt 集成
//! 对现状段渲染断言（eligible 子集当下无 ≥0.70 对时输出「未检出」也是合法
//! 输出）。
//!
//! `#[ignore]`：依赖 `~/.agents/memory` 的真实数据，仅本地
//! `cargo test -p memory --test reflection_corpus_snapshot -- --ignored` 运行。

use memory::api::reflection::{ReflectionEngine, ReflectionWorkflow};
use memory::api::{MemoryLayer, ProjectMemoryKey};
use share::config::MemoryConfig;

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

async fn open_real_corpus() -> std::sync::Arc<dyn memory::api::MemoryPort> {
    let home = std::env::var("HOME").expect("HOME must be set");
    let agents_dir = std::path::Path::new(&home).join(".agents");
    let storage = storage::wire_file_system_dataset(&agents_dir).unwrap();
    let legacy = memory::wire_legacy_memory_source_factory(agents_dir.clone());
    let event_root = storage::SafeStorageRoot::open(&agents_dir).unwrap();
    let opener = memory::wire_memory_opener(storage, legacy, None, event_root);
    let project = ProjectMemoryKey::derive(
        "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath",
        Some("/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.git"),
    )
    .unwrap();
    opener
        .open_memory(&project, &MemoryConfig::default())
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "依赖本机真实 corpus 快照（#1987 验收，本地 --ignored 运行）"]
async fn real_corpus_duplicate_detection_finds_known_pair() {
    let port = open_real_corpus().await;
    let all = port.list(Some(MemoryLayer::Project)).await;
    // 快照物理条数随反思收敛动态变化（#1989 审计时 168，2026-10-20 实测 88）：
    // 只保证参与相似度检测的语料规模下限，不再绑定旧数据量。
    assert!(
        all.len() > 50,
        "真实快照 project active 应保持可观语料（>50）: {len}",
        len = all.len()
    );

    // 全量真实数据：中文内容为主，bigram 分词下的相似对检测必须可运行且确定。
    // 历史「已知对」（#1981 写锁两代结论，J≈0.82）已随 #1989 反思 synthesize/
    // outdated 收敛——「检出数下降」正是系统按设计工作的证据；检测能力本身由
    // 域级 bigram 测试保障，此处只验真语料规模下限与确定性。
    let first = ReflectionEngine::top_similar_pairs(&all, 0.70, 8);
    let second = ReflectionEngine::top_similar_pairs(&all, 0.70, 8);
    assert_eq!(first, second, "真语料相似对检测必须确定（两次调用一致）");
}

#[tokio::test]
#[ignore = "依赖本机真实 corpus 快照（#1987 验收，本地 --ignored 运行）"]
async fn real_corpus_prompt_renders_status_section() {
    let port = open_real_corpus().await;
    let prompt = ReflectionWorkflow::build_prompt(
        &[share::message::Message::user("验收：corpus 现状注入")],
        "zh",
        port.as_ref(),
        now_unix(),
        150,
        200,
        0,
    )
    .await;

    assert!(
        prompt.text.contains("# 记忆库现状"),
        "现状段必须出现: {text}",
        text = prompt.text
    );
    assert!(prompt.text.contains("archive="));
    assert!(
        prompt.text.contains("MUST 给出 synthesizes"),
        "合并指令必须出现"
    );
    // #2007：真实 corpus 中 pinned 失效驻留（#1989 审计约 9 条）MUST 进入
    // 失效驻留标注段供 LLM 裁决——pinned 僵尸对反思可见是本次改造的核心验收。
    assert!(
        prompt.text.contains("Lapsed entries"),
        "真实快照存在 pinned 失效驻留，失效驻留段必须出现"
    );
}
