//! 本地 aemeath 二进制变化检测。
//!
//! 运行中的进程持有旧 inode；`build_cli.sh` 等安装流程用原子 rename 替换安装路径
//! 上的文件。这里以 `current_exe()` 解析后的真实文件为基线，周期比对文件身份，
//! 变化时提示「重启生效」。
//!
//! 路径绝不硬编码：自定义 `INSTALL_DIR`、`cargo run` 的 target 路径与软链启动都按
//! `current_exe()` + `canonicalize()` 的实际结果检测。全链 fail-open——解析或
//! stat 失败只表示「不检测」，绝不影响 TUI。

use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use super::constants::LOCAL_BIN_CHECK_INTERVAL;

/// 文件身份指纹：任一字段变化都表示磁盘上的二进制已被替换。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BinFingerprint {
    size: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl BinFingerprint {
    fn from_metadata(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        Self {
            size: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            dev: metadata.dev(),
            #[cfg(unix)]
            ino: metadata.ino(),
        }
    }
}

/// 本地二进制指纹变化检测器。
pub(crate) struct LocalBinWatch {
    path: Option<PathBuf>,
    baseline: Option<BinFingerprint>,
    last_checked: Option<Instant>,
}

impl LocalBinWatch {
    /// 以当前进程的二进制为检测目标。
    pub(crate) fn from_current_exe() -> Self {
        Self::new(current_bin_path())
    }

    fn new(path: Option<PathBuf>) -> Self {
        let baseline = path.as_deref().and_then(read_fingerprint);
        Self {
            path,
            baseline,
            last_checked: None,
        }
    }

    /// 节流轮询；检测到变化时返回该路径，并用新指纹覆盖基线（避免重复提醒）。
    pub(crate) fn poll(&mut self, now: Instant) -> Option<PathBuf> {
        let path = self.path.clone()?;
        if let Some(last_checked) = self.last_checked {
            if now.duration_since(last_checked) < LOCAL_BIN_CHECK_INTERVAL {
                return None;
            }
        }
        self.last_checked = Some(now);
        let current = read_fingerprint(&path)?;
        let changed = self.baseline.is_some_and(|baseline| baseline != current);
        self.baseline = Some(current);
        changed.then_some(path)
    }
}

impl Default for LocalBinWatch {
    fn default() -> Self {
        Self::from_current_exe()
    }
}

/// 当前进程二进制路径（解析软链后的真实文件）。
fn current_bin_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(std::fs::canonicalize(&exe).unwrap_or(exe))
}

fn read_fingerprint(path: &Path) -> Option<BinFingerprint> {
    std::fs::metadata(path)
        .ok()
        .map(|metadata| BinFingerprint::from_metadata(&metadata))
}

#[cfg(test)]
#[path = "local_bin_watch_tests.rs"]
mod tests;
