//! 读路径校验：目录形态、manifest 契约与逐资产 open → 同一 fd 元数据 → 流式 SHA-256。
//!
//! 精确的防护边界（勿夸大为「无竞态」）：
//! - **末段文件**：一律 NO-follow 打开（unix 用 `O_NOFOLLOW`，末段符号链接直接打开失败），
//!   常规文件判定、字节数与读取全部取自**打开后的同一 fd**（`fstat` + 同 fd 读），
//!   因此末段在打开前被换成链接也绝不跟随。
//! - **父目录组件**（如 `tokenizer/`）：逐组件 `symlink_metadata` 探测并拒绝符号链接。
//!   这是「先探测、后按路径打开」的逐组件校验，探测与打开之间**存在竞态窗口**
//!   （TOCTOU），本模块 **NEVER** 声称父组件校验无竞态，也未引入 capability dir。
//! - **完整性兜底**：竞态窗口内即使父组件被替换，逐资产长度 + 流式 SHA-256 与
//!   canonical manifest 逐项比对会发现任何内容变化——完整性最终由 SHA-256 锁定。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::constants::MODEL_MANIFEST_FILE_NAME;
use crate::domain::{ModelAsset, ModelManifest, ModelManifestError};
use crate::ports::{InstalledAssets, InvalidAssetKind};

/// 校验失败（内部传递）：typed 类别 + 中文 detail。
#[derive(Debug)]
pub(super) struct AssetVerificationFailure {
    /// 失败类别（损坏 / 不可读 / 不支持）。
    pub(super) kind: InvalidAssetKind,
    /// 中文失败原因。
    pub(super) detail: String,
}

impl AssetVerificationFailure {
    /// 组装一条校验失败。
    pub(super) fn new(kind: InvalidAssetKind, detail: String) -> Self {
        Self { kind, detail }
    }
}

/// final revision 目录的安装前探测结果。
pub(super) enum FinalRevisionState {
    /// 目标不存在，可安全 no-replace rename。
    Absent,
    /// 目标已存在且校验通过（幂等）。
    Valid(InstalledAssets),
    /// 目标存在但校验无效（拒绝覆盖）。
    Invalid { detail: String },
    /// 目标存在但探测即 IO 失败（既不判定无效，也 **NEVER** 覆盖）。
    Unprobeable { detail: String },
}

/// 安装前探测 final revision：不存在 / 有效（幂等）/ 无效（拒绝覆盖）/ 不可探测。
pub(super) fn probe_final_revision(
    revision_dir: &Path,
    expected_manifest: &ModelManifest,
) -> FinalRevisionState {
    match std::fs::symlink_metadata(revision_dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FinalRevisionState::Absent,
        Err(error) => FinalRevisionState::Unprobeable {
            detail: format!("安装目标目录不可探测：{error}"),
        },
        Ok(_) => match verify_installed_directory(revision_dir, expected_manifest) {
            Ok(installed) => FinalRevisionState::Valid(installed),
            Err(failure) => FinalRevisionState::Invalid {
                detail: failure.detail,
            },
        },
    }
}

/// 完整校验一个安装目录（manifest 契约 + 逐资产长度/流式 SHA-256）。
pub(super) fn verify_installed_directory(
    directory: &Path,
    expected_manifest: &ModelManifest,
) -> Result<InstalledAssets, AssetVerificationFailure> {
    verify_directory_entry(directory)?;
    verify_manifest_file(directory, expected_manifest)?;
    verify_asset_files(directory, expected_manifest)?;
    InstalledAssets::new(expected_manifest.clone(), directory.to_path_buf()).map_err(|error| {
        AssetVerificationFailure::new(InvalidAssetKind::Unsupported, error.to_string())
    })
}

/// 目录本体：存在、非常规 symlink、是目录。
fn verify_directory_entry(directory: &Path) -> Result<(), AssetVerificationFailure> {
    let metadata = std::fs::symlink_metadata(directory).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AssetVerificationFailure::new(
                InvalidAssetKind::Corrupt,
                format!("安装目录不存在：{}", directory.display()),
            )
        } else {
            AssetVerificationFailure::new(
                InvalidAssetKind::Unreadable,
                format!("安装目录不可探测：{error}"),
            )
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(AssetVerificationFailure::new(
            InvalidAssetKind::Corrupt,
            format!("安装目录是可疑符号链接：{}", directory.display()),
        ));
    }
    if !metadata.is_dir() {
        return Err(AssetVerificationFailure::new(
            InvalidAssetKind::Unsupported,
            format!("安装路径不是目录：{}", directory.display()),
        ));
    }
    Ok(())
}

/// manifest 文件：NO-follow 打开、同一 fd 判定常规文件、JSON 可解析、契约通过、
/// revision 与目录目标一致、且与期望 manifest 完全一致（单一期望真相）。
fn verify_manifest_file(
    directory: &Path,
    expected_manifest: &ModelManifest,
) -> Result<(), AssetVerificationFailure> {
    let manifest_path = directory.join(MODEL_MANIFEST_FILE_NAME);
    let mut manifest_file = open_no_follow(&manifest_path).map_err(|failure| match failure {
        OpenFailure::Missing => AssetVerificationFailure::new(
            InvalidAssetKind::Corrupt,
            format!("安装目录缺少 {MODEL_MANIFEST_FILE_NAME}（半成品安装）"),
        ),
        OpenFailure::Symlink => AssetVerificationFailure::new(
            InvalidAssetKind::Corrupt,
            format!("安装目录中的 {MODEL_MANIFEST_FILE_NAME} 是可疑符号链接"),
        ),
        OpenFailure::Io(error) => AssetVerificationFailure::new(
            InvalidAssetKind::Unreadable,
            format!("{MODEL_MANIFEST_FILE_NAME} 不可读：{error}"),
        ),
    })?;
    let metadata = manifest_file.metadata().map_err(|error| {
        AssetVerificationFailure::new(
            InvalidAssetKind::Unreadable,
            format!("{MODEL_MANIFEST_FILE_NAME} 元数据不可读：{error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(AssetVerificationFailure::new(
            InvalidAssetKind::Unsupported,
            format!("{MODEL_MANIFEST_FILE_NAME} 不是常规文件（布局不符）"),
        ));
    }
    let mut source = String::new();
    manifest_file.read_to_string(&mut source).map_err(|error| {
        AssetVerificationFailure::new(
            InvalidAssetKind::Unreadable,
            format!("{MODEL_MANIFEST_FILE_NAME} 不可读：{error}"),
        )
    })?;
    let installed_manifest = ModelManifest::parse(&source).map_err(|error| match error {
        ModelManifestError::Schema(_) => AssetVerificationFailure::new(
            InvalidAssetKind::Corrupt,
            format!("{MODEL_MANIFEST_FILE_NAME} 不是合法 JSON：{error}"),
        ),
        other => AssetVerificationFailure::new(
            InvalidAssetKind::Unsupported,
            format!("manifest 契约校验失败：{other}"),
        ),
    })?;
    if installed_manifest.engine_revision != expected_manifest.engine_revision {
        return Err(AssetVerificationFailure::new(
            InvalidAssetKind::Unsupported,
            format!(
                "落盘 manifest 的 revision {} 与目标 revision {} 不符",
                installed_manifest.engine_revision, expected_manifest.engine_revision
            ),
        ));
    }
    if installed_manifest != *expected_manifest {
        return Err(AssetVerificationFailure::new(
            InvalidAssetKind::Unsupported,
            "落盘 manifest 与期望 manifest 不一致（资产声明与当前发行契约不符）".to_owned(),
        ));
    }
    Ok(())
}

/// 逐资产校验：父目录逐组件拒链接、NO-follow 打开、同一 fd 判定常规文件与字节数、
/// 流式 SHA-256。
fn verify_asset_files(
    directory: &Path,
    expected_manifest: &ModelManifest,
) -> Result<(), AssetVerificationFailure> {
    for asset in &expected_manifest.assets {
        verify_asset_file(directory, asset)?;
    }
    Ok(())
}

fn verify_asset_file(directory: &Path, asset: &ModelAsset) -> Result<(), AssetVerificationFailure> {
    // 资产路径来自已校验 manifest（安全相对路径），绝不接受外部拼接的绝对目标。
    verify_asset_parent_directories(directory, &asset.path)?;
    let asset_path = directory.join(&asset.path);
    // 先 NO-follow 打开，再取同一 fd 的元数据判定 regular + 字节数：
    // 链接在「探测 → 打开」之间被替换也绝不跟随。
    let file = open_no_follow(&asset_path).map_err(|failure| match failure {
        OpenFailure::Missing => AssetVerificationFailure::new(
            InvalidAssetKind::Corrupt,
            format!("资产缺失：{}", asset.path),
        ),
        OpenFailure::Symlink => AssetVerificationFailure::new(
            InvalidAssetKind::Corrupt,
            format!("资产 {} 是可疑符号链接", asset.path),
        ),
        OpenFailure::Io(error) => AssetVerificationFailure::new(
            InvalidAssetKind::Unreadable,
            format!("资产 {} 不可读：{error}", asset.path),
        ),
    })?;
    let metadata = file.metadata().map_err(|error| {
        AssetVerificationFailure::new(
            InvalidAssetKind::Unreadable,
            format!("资产 {} 元数据不可读：{error}", asset.path),
        )
    })?;
    if !metadata.is_file() {
        return Err(AssetVerificationFailure::new(
            InvalidAssetKind::Unsupported,
            format!("资产 {} 不是常规文件（布局不符）", asset.path),
        ));
    }
    if metadata.len() != asset.byte_length {
        return Err(AssetVerificationFailure::new(
            InvalidAssetKind::Corrupt,
            format!(
                "资产 {} 字节数与 manifest 不符（期望 {}，实际 {}）",
                asset.path,
                asset.byte_length,
                metadata.len()
            ),
        ));
    }
    // 775MB 级 GGUF：流式分块哈希，NEVER read_to_end。
    let digest = utils::sha256_reader_hex(file).map_err(|error| {
        AssetVerificationFailure::new(
            InvalidAssetKind::Unreadable,
            format!("资产 {} 读取失败：{error}", asset.path),
        )
    })?;
    if digest != asset.sha256 {
        return Err(AssetVerificationFailure::new(
            InvalidAssetKind::Corrupt,
            format!(
                "资产 {} 的 SHA-256 与 manifest 不符（期望 {}，实际 {digest}）",
                asset.path, asset.sha256
            ),
        ));
    }
    Ok(())
}

/// 资产相对路径的中间目录（如 `tokenizer/`）逐组件校验：
/// 每一级都必须是真实存在的目录，符号链接一律拒绝，
/// 校验器 **NEVER** 有意穿过中间目录的链接。
/// 注意：本探测与随后的按路径打开之间存在 TOCTOU 竞态窗口（见文件头注释），
/// 竞态内的任何替换最终由逐资产长度 + SHA-256 与 manifest 比对锁定。
fn verify_asset_parent_directories(
    directory: &Path,
    relative: &str,
) -> Result<(), AssetVerificationFailure> {
    let Some(parent) = Path::new(relative).parent() else {
        return Ok(());
    };
    let mut current = directory.to_path_buf();
    for component in parent.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(AssetVerificationFailure::new(
                InvalidAssetKind::Unsupported,
                format!("资产 {relative} 路径包含非法组件"),
            ));
        };
        current.push(segment);
        match std::fs::symlink_metadata(&current) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(AssetVerificationFailure::new(
                    InvalidAssetKind::Corrupt,
                    format!("资产 {relative} 的父目录缺失：{}", current.display()),
                ));
            }
            Err(error) => {
                return Err(AssetVerificationFailure::new(
                    InvalidAssetKind::Unreadable,
                    format!("资产 {relative} 的父目录不可探测：{error}"),
                ));
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AssetVerificationFailure::new(
                    InvalidAssetKind::Corrupt,
                    format!(
                        "资产 {relative} 的父目录是可疑符号链接：{}",
                        current.display()
                    ),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(AssetVerificationFailure::new(
                    InvalidAssetKind::Unsupported,
                    format!("资产 {relative} 的父目录不是目录：{}", current.display()),
                ));
            }
            Ok(_) => {}
        }
    }
    Ok(())
}

/// NO-follow 打开失败的分类：路径缺失 / 末段符号链接 / 其它 IO 错误。
enum OpenFailure {
    /// 目标不存在。
    Missing,
    /// 末段是符号链接（NO-follow 打开被拒绝）。
    Symlink,
    /// 其它 IO 错误（权限、元数据读取失败等）。
    Io(std::io::Error),
}

/// NO-follow 打开：unix 用 `O_NOFOLLOW`（末段符号链接直接失败，
/// 不进入「先探测后打开」的跟随窗口）；其余平台先 `symlink_metadata` 拒链接再打开。
#[cfg(unix)]
fn open_no_follow(path: &Path) -> Result<File, OpenFailure> {
    use std::os::unix::fs::OpenOptionsExt;
    // O_NOFOLLOW 不由 std 暴露；值取自 rustix 的平台常量（与 libc 同一 ABI 定义）。
    let no_follow = rustix::fs::OFlags::NOFOLLOW.bits() as i32;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(no_follow)
        .open(path)
        .map_err(|error| classify_open_error(path, error))
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> Result<File, OpenFailure> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(OpenFailure::Missing),
        Err(error) => Err(OpenFailure::Io(error)),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(OpenFailure::Symlink),
        Ok(_) => std::fs::File::open(path).map_err(|error| classify_open_error(path, error)),
    }
}

/// 打开错误归类：`O_NOFOLLOW` 把末段符号链接折成 ELOOP 类失败，
/// 复核链接态归为 `Symlink`，避免把攻击形态误报为普通不可读。
fn classify_open_error(path: &Path, error: std::io::Error) -> OpenFailure {
    match error.kind() {
        std::io::ErrorKind::NotFound => OpenFailure::Missing,
        _ => match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => OpenFailure::Symlink,
            _ => OpenFailure::Io(error),
        },
    }
}
