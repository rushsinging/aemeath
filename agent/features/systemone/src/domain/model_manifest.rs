//! System One 模型 manifest：schema 契约、平台支持与资产安全校验。
//!
//! manifest 是下载与本地存储的**单一信息来源**：资产路径、URL、字节数与
//! SHA-256 只在这里声明与校验；adapter 与 application 只消费校验通过的结构。
//! 纯领域：无 IO、不读环境变量；目标平台由调用方传入 `validate_for`。

use std::collections::HashSet;

use crate::constants::{
    MODEL_ENGINE_REVISION_MAX_LEN, MODEL_GGUF_FILE_NAME, MODEL_HIDDEN_SIZE,
    MODEL_MANIFEST_SCHEMA_VERSION, MODEL_POINTER_DIMENSION, MODEL_REQUIRED_PLATFORM,
    POINTER_HEAD_FILE_NAME, TOKENIZER_DIR_PREFIX,
};

/// manifest 解析或契约校验失败（fail-closed：任一违规即拒绝整份 manifest）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelManifestError {
    /// JSON 解析失败：缺字段、未知字段或类型不符。
    Schema(String),
    /// schema 版本不是当前版本。
    UnsupportedSchemaVersion { found: u32, expected: u32 },
    /// engine_revision 为空白。
    EmptyEngineRevision,
    /// engine_revision 不是安全的安装目录 canonical segment。
    InvalidEngineRevision { revision: String },
    /// hidden size 不是生产契约值。
    UnsupportedHiddenSize { found: u32, expected: u32 },
    /// pointer dimension 不是生产契约值。
    UnsupportedPointerDimension { found: u32, expected: u32 },
    /// temperature 非有限或非正。
    InvalidTemperature,
    /// 支持平台列表为空。
    EmptySupportedPlatforms,
    /// 支持平台列表缺少必选平台。
    RequiredPlatformMissing { platform: String },
    /// 平台标识为空白。
    InvalidPlatformLabel { platform: String },
    /// 目标平台不在支持列表内。
    UnsupportedPlatform { platform: String },
    /// 资产路径为空。
    EmptyAssetPath,
    /// 资产路径指向目录（以 `/` 结尾）。
    DirectoryAssetPath { path: String },
    /// 资产路径非安全相对路径（绝对路径、`.`/`..`、空组件）。
    UnsafeAssetPath { path: String },
    /// 资产路径重复（按 ASCII lowercase 判定物理冲突）。
    DuplicateAssetPath { path: String },
    /// 资产路径不在严格生产布局内（仅允许 model.gguf、pointer_head.safetensors 与 tokenizer/ 下文件）。
    UnexpectedAssetPath { path: String },
    /// 资产 URL 非法（非 https、空或含空白）。
    InvalidAssetUrl { path: String },
    /// SHA-256 非 64 位小写十六进制。
    InvalidAssetSha256 { path: String },
    /// 资产字节数为 0。
    ZeroAssetByteLength { path: String },
    /// `model.gguf` 资产数量不等于 1。
    ModelGgufCount { found: usize },
    /// `pointer_head.safetensors` 资产数量不等于 1。
    PointerHeadAssetCount { found: usize },
    /// 缺少 `tokenizer/` 目录下的文件。
    MissingTokenizerAsset,
}

impl std::fmt::Display for ModelManifestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema(detail) => write!(formatter, "manifest 解析失败：{detail}"),
            Self::UnsupportedSchemaVersion { found, expected } => write!(
                formatter,
                "manifest schema 版本应为 {expected}，实际为 {found}"
            ),
            Self::EmptyEngineRevision => write!(formatter, "manifest 的 engine_revision 不能为空"),
            Self::InvalidEngineRevision { revision } => write!(
                formatter,
                "engine_revision {revision:?} 不是安全的安装目录段（须以字母数字开头、仅含 [A-Za-z0-9._-]、最长 64 且不含 ..）"
            ),
            Self::UnsupportedHiddenSize { found, expected } => {
                write!(formatter, "hidden size 应为 {expected}，实际为 {found}")
            }
            Self::UnsupportedPointerDimension { found, expected } => write!(
                formatter,
                "pointer dimension 应为 {expected}，实际为 {found}"
            ),
            Self::InvalidTemperature => write!(formatter, "temperature 必须为有限正数"),
            Self::EmptySupportedPlatforms => write!(formatter, "manifest 的支持平台列表不能为空"),
            Self::RequiredPlatformMissing { platform } => {
                write!(formatter, "支持平台列表缺少必选平台 {platform}")
            }
            Self::InvalidPlatformLabel { platform } => {
                write!(formatter, "平台标识不能为空白：{platform:?}")
            }
            Self::UnsupportedPlatform { platform } => {
                write!(formatter, "当前平台 {platform} 不在模型支持列表内")
            }
            Self::EmptyAssetPath => write!(formatter, "资产路径不能为空"),
            Self::DirectoryAssetPath { path } => {
                write!(formatter, "资产路径 {path:?} 指向目录，必须是文件路径")
            }
            Self::UnsafeAssetPath { path } => write!(
                formatter,
                "资产路径 {path:?} 不是安全相对路径（禁止绝对路径、..、反斜杠、空白与非 ASCII 字符）"
            ),
            Self::DuplicateAssetPath { path } => {
                write!(formatter, "资产路径重复：{path:?}")
            }
            Self::UnexpectedAssetPath { path } => write!(
                formatter,
                "资产路径 {path:?} 不在生产布局内（仅允许 model.gguf、pointer_head.safetensors 与 tokenizer/ 下文件）"
            ),
            Self::InvalidAssetUrl { path } => write!(
                formatter,
                "资产 {path:?} 的 URL 非法（必须为非空 https 地址，且不含空白）"
            ),
            Self::InvalidAssetSha256 { path } => write!(
                formatter,
                "资产 {path:?} 的 SHA-256 必须是 64 位小写十六进制"
            ),
            Self::ZeroAssetByteLength { path } => {
                write!(formatter, "资产 {path:?} 的字节数必须大于 0")
            }
            Self::ModelGgufCount { found } => {
                write!(formatter, "model.gguf 资产数量应为 1，实际为 {found}")
            }
            Self::PointerHeadAssetCount { found } => write!(
                formatter,
                "pointer_head.safetensors 资产数量应为 1，实际为 {found}"
            ),
            Self::MissingTokenizerAsset => {
                write!(formatter, "缺少 tokenizer 目录下的资产文件（至少一个）")
            }
        }
    }
}

impl std::error::Error for ModelManifestError {}

/// 单个可下载资产：安全相对路径、下载 URL、字节数与 SHA-256。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAsset {
    /// 相对安装根目录的安全路径（禁止绝对路径、`.`/`..` 与目录形态）。
    pub path: String,
    /// https 下载地址。
    pub url: String,
    /// 期望字节数（必须大于 0）。
    pub byte_length: u64,
    /// 期望 SHA-256（64 位小写十六进制）。
    pub sha256: String,
}

/// 模型 manifest：下载与本地存储的单一信息来源（未知字段一律拒绝）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelManifest {
    /// manifest schema 版本，MUST 等于当前版本。
    pub schema_version: u32,
    /// 引擎/模型 revision，安装目录与审计以此为准。
    pub engine_revision: String,
    /// 模型 hidden size，MUST 为 1024。
    pub hidden_size: u32,
    /// PointerHead 维度，MUST 为 256。
    pub pointer_dimension: u32,
    /// 评分温度，MUST 为有限正数。
    pub temperature: f32,
    /// 支持的平台标识列表，非空且 MUST 含必选平台。
    pub supported_platforms: Vec<String>,
    /// 资产清单（model.gguf、pointer_head.safetensors、tokenizer/ 等）。
    pub assets: Vec<ModelAsset>,
}

/// 宽松 schema 探针：只读 `schema_version`、容忍未知字段。
///
/// 在 `deny_unknown_fields` 的完整解析**之前**判定版本，确保「v2 + 新字段」
/// 返回 `UnsupportedSchemaVersion` 而不是 generic `Schema`；缺 `schema_version` 仍为 `Schema`。
#[derive(serde::Deserialize)]
struct SchemaProbe {
    schema_version: u32,
}

/// 首批必选平台标识（manifest 支持平台列表 MUST 包含它）。
///
/// 单一真相入口：crate 外与跨模块调用方经此函数获取必选平台；
/// crate 内的契约校验可直接引用同一内部常量（同源，无第二份取值）。
pub fn required_platform() -> &'static str {
    MODEL_REQUIRED_PLATFORM
}

impl ModelManifest {
    /// 解析 manifest JSON 并执行全部契约校验（平台校验除外，由 `validate_for` 承担）。
    ///
    /// 先用宽松 `SchemaProbe` 判定 schema 版本，再执行 `deny_unknown_fields` 完整解析。
    pub fn parse(source: &str) -> Result<Self, ModelManifestError> {
        let probe: SchemaProbe = serde_json::from_str(source)
            .map_err(|error| ModelManifestError::Schema(error.to_string()))?;
        if probe.schema_version != MODEL_MANIFEST_SCHEMA_VERSION {
            return Err(ModelManifestError::UnsupportedSchemaVersion {
                found: probe.schema_version,
                expected: MODEL_MANIFEST_SCHEMA_VERSION,
            });
        }
        let manifest: Self = serde_json::from_str(source)
            .map_err(|error| ModelManifestError::Schema(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// 校验 schema 版本、engine_revision 目录段、生产维度、温度、平台列表与全部资产规则。
    pub fn validate(&self) -> Result<(), ModelManifestError> {
        if self.schema_version != MODEL_MANIFEST_SCHEMA_VERSION {
            return Err(ModelManifestError::UnsupportedSchemaVersion {
                found: self.schema_version,
                expected: MODEL_MANIFEST_SCHEMA_VERSION,
            });
        }
        if self.engine_revision.trim().is_empty() {
            return Err(ModelManifestError::EmptyEngineRevision);
        }
        if !is_canonical_engine_revision(&self.engine_revision) {
            return Err(ModelManifestError::InvalidEngineRevision {
                revision: self.engine_revision.clone(),
            });
        }
        if self.hidden_size != MODEL_HIDDEN_SIZE {
            return Err(ModelManifestError::UnsupportedHiddenSize {
                found: self.hidden_size,
                expected: MODEL_HIDDEN_SIZE,
            });
        }
        if self.pointer_dimension != MODEL_POINTER_DIMENSION {
            return Err(ModelManifestError::UnsupportedPointerDimension {
                found: self.pointer_dimension,
                expected: MODEL_POINTER_DIMENSION,
            });
        }
        if !self.temperature.is_finite() || self.temperature <= 0.0 {
            return Err(ModelManifestError::InvalidTemperature);
        }
        if self.supported_platforms.is_empty() {
            return Err(ModelManifestError::EmptySupportedPlatforms);
        }
        for platform in &self.supported_platforms {
            if platform.is_empty()
                || platform
                    .chars()
                    .any(|character| character.is_whitespace() || character.is_control())
            {
                return Err(ModelManifestError::InvalidPlatformLabel {
                    platform: platform.clone(),
                });
            }
        }
        if !self
            .supported_platforms
            .iter()
            .any(|supported| supported == MODEL_REQUIRED_PLATFORM)
        {
            return Err(ModelManifestError::RequiredPlatformMissing {
                platform: required_platform().to_owned(),
            });
        }
        self.validate_assets()?;
        Ok(())
    }

    /// 校验全部契约，并要求目标平台在支持列表内（平台由调用方传入，domain 不读环境）。
    pub fn validate_for(&self, platform: &str) -> Result<(), ModelManifestError> {
        self.validate()?;
        if !self
            .supported_platforms
            .iter()
            .any(|supported| supported == platform)
        {
            return Err(ModelManifestError::UnsupportedPlatform {
                platform: platform.to_owned(),
            });
        }
        Ok(())
    }

    /// 已校验结构访问器：唯一的 `model.gguf` 资产（缺失/多余返回角色数量错误）。
    pub fn model_asset(&self) -> Result<&ModelAsset, ModelManifestError> {
        let model_assets: Vec<&ModelAsset> = self
            .assets
            .iter()
            .filter(|asset| asset.path == MODEL_GGUF_FILE_NAME)
            .collect();
        match model_assets.as_slice() {
            [model_asset] => Ok(model_asset),
            _ => Err(ModelManifestError::ModelGgufCount {
                found: model_assets.len(),
            }),
        }
    }

    /// 已校验结构访问器：唯一的根路径 `pointer_head.safetensors` 资产。
    pub fn pointer_head_asset(&self) -> Result<&ModelAsset, ModelManifestError> {
        let pointer_assets: Vec<&ModelAsset> = self
            .assets
            .iter()
            .filter(|asset| asset.path == POINTER_HEAD_FILE_NAME)
            .collect();
        match pointer_assets.as_slice() {
            [pointer_asset] => Ok(pointer_asset),
            _ => Err(ModelManifestError::PointerHeadAssetCount {
                found: pointer_assets.len(),
            }),
        }
    }

    /// 已校验结构访问器：`tokenizer/` 目录下的全部资产（缺失返回 MissingTokenizerAsset）。
    pub fn tokenizer_assets(&self) -> Result<Vec<&ModelAsset>, ModelManifestError> {
        let tokenizer_assets: Vec<&ModelAsset> = self
            .assets
            .iter()
            .filter(|asset| asset.path.starts_with(TOKENIZER_DIR_PREFIX))
            .collect();
        if tokenizer_assets.is_empty() {
            Err(ModelManifestError::MissingTokenizerAsset)
        } else {
            Ok(tokenizer_assets)
        }
    }

    /// 校验资产安全路径、按 ASCII lowercase 去重、URL、字节数、SHA-256、
    /// 三类角色数量契约与严格生产布局。
    fn validate_assets(&self) -> Result<(), ModelManifestError> {
        let mut seen_paths: HashSet<String> = HashSet::new();
        let mut gguf_count = 0_usize;
        let mut pointer_head_count = 0_usize;
        let mut tokenizer_count = 0_usize;
        for asset in &self.assets {
            validate_asset_path(&asset.path)?;
            if !seen_paths.insert(asset.path.to_ascii_lowercase()) {
                return Err(ModelManifestError::DuplicateAssetPath {
                    path: asset.path.clone(),
                });
            }
            if !is_download_url(&asset.url) {
                return Err(ModelManifestError::InvalidAssetUrl {
                    path: asset.path.clone(),
                });
            }
            if asset.byte_length == 0 {
                return Err(ModelManifestError::ZeroAssetByteLength {
                    path: asset.path.clone(),
                });
            }
            if !is_canonical_sha256(&asset.sha256) {
                return Err(ModelManifestError::InvalidAssetSha256 {
                    path: asset.path.clone(),
                });
            }
            // 角色计数按精确根路径判定：nested 同名文件不算角色。
            if asset.path == MODEL_GGUF_FILE_NAME {
                gguf_count += 1;
            }
            if asset.path == POINTER_HEAD_FILE_NAME {
                pointer_head_count += 1;
            }
            if asset.path.starts_with(TOKENIZER_DIR_PREFIX) {
                tokenizer_count += 1;
            }
        }
        if gguf_count != 1 {
            return Err(ModelManifestError::ModelGgufCount { found: gguf_count });
        }
        if pointer_head_count != 1 {
            return Err(ModelManifestError::PointerHeadAssetCount {
                found: pointer_head_count,
            });
        }
        if tokenizer_count == 0 {
            return Err(ModelManifestError::MissingTokenizerAsset);
        }
        for asset in &self.assets {
            if !is_production_asset_path(&asset.path) {
                return Err(ModelManifestError::UnexpectedAssetPath {
                    path: asset.path.clone(),
                });
            }
        }
        Ok(())
    }
}

/// 严格生产布局：仅允许根 `model.gguf`、根 `pointer_head.safetensors` 与 `tokenizer/` 下文件。
fn is_production_asset_path(path: &str) -> bool {
    path == MODEL_GGUF_FILE_NAME
        || path == POINTER_HEAD_FILE_NAME
        || path.starts_with(TOKENIZER_DIR_PREFIX)
}

/// 校验资产路径为安全相对文件路径（非空、非绝对、无 `.`/`..`、非目录），
/// 且字符集限定为 ASCII 字母数字与 `-_.`、`/`（拒绝反斜杠、空白、控制字符与非 ASCII）。
fn validate_asset_path(path: &str) -> Result<(), ModelManifestError> {
    if path.is_empty() {
        return Err(ModelManifestError::EmptyAssetPath);
    }
    if path.starts_with('/') {
        return Err(ModelManifestError::UnsafeAssetPath {
            path: path.to_owned(),
        });
    }
    if path.ends_with('/') {
        return Err(ModelManifestError::DirectoryAssetPath {
            path: path.to_owned(),
        });
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(ModelManifestError::UnsafeAssetPath {
                path: path.to_owned(),
            });
        }
    }
    if !path.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/')
    }) {
        return Err(ModelManifestError::UnsafeAssetPath {
            path: path.to_owned(),
        });
    }
    Ok(())
}

/// URL MUST 为 https scheme、主机非空且不含空白/控制字符，
/// 并拒绝 query-only / fragment-only 形态的空主机。
fn is_download_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    if rest.is_empty()
        || rest
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return false;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty()
}

/// engine_revision MUST 是安全的安装目录 canonical segment：
/// ASCII `[A-Za-z0-9][A-Za-z0-9._-]{0,63}` 且整体不含 `..`（空白由 `EmptyEngineRevision` 归口）。
fn is_canonical_engine_revision(revision: &str) -> bool {
    let bytes = revision.as_bytes();
    let Some((first_byte, tail)) = bytes.split_first() else {
        return false;
    };
    bytes.len() <= MODEL_ENGINE_REVISION_MAX_LEN
        && first_byte.is_ascii_alphanumeric()
        && !revision.contains("..")
        && tail
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
}

/// SHA-256 MUST 恰为 64 位小写 ASCII 十六进制（规范为小写）。
fn is_canonical_sha256(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "model_manifest_tests.rs"]
mod tests;
