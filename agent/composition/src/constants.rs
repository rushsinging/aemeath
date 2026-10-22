//! crate 身份常量（#1146 双轨归位）。

pub(crate) const LOG_TARGET: &str = "aemeath:composition";

/// System One 当前发行 revision（安装目录名与升级检测的单一真相）。
pub(crate) const SYSTEMONE_RELEASE_REVISION: &str = "kev-0.8b-q8-r1";

/// System One 发行仓库 base URL（三资产按安装路径平铺于 repo 根）。
pub(crate) const SYSTEMONE_RELEASE_BASE_URL: &str =
    "https://huggingface.co/rushsinging/aemeath-systemone-kev/resolve/main";

/// System One 发行资产（长度与 SHA-256 来自 2026-10-09 上传的真实文件）。
pub(crate) const SYSTEMONE_RELEASE_ASSETS: [(&str, u64, &str); 3] = [
    (
        "model.gguf",
        811_843_104,
        "5733ee5bccca3b8d5790581c6f41b7e25e13f7ffe38a3105070f8b0e7aa6b439",
    ),
    (
        "pointer_head.safetensors",
        2_099_538,
        "98887fc07d67b2c372d392c56fd328f5f5c0d8005e6d3f04a8b833f0e282e824",
    ),
    (
        "tokenizer/tokenizer.json",
        19_989_339,
        "a5cd9732badce41de57e6efce8302930ded1c1188c5f81feb2bd6c24c4a1941f",
    ),
];

/// HF xet / LFS CDN 重定向 host 白名单（`resolve/main` 302 目标域；manifest
/// URL host `huggingface.co` 自动进白名单，无需列此处）。
pub(crate) const HF_CDN_REDIRECT_HOSTS: [&str; 4] = [
    "us.aws.cdn.hf.co",
    "cas-bridge.xethub.hf.co",
    "transfer.xethub.hf.co",
    "cdn-lfs.hf.co",
];
