/// Compact 进度模型。
///
/// 由 runtime 的 `CompactProgress` 事件驱动，TUI 据此渲染 spinner 行内嵌进度条。
#[derive(Clone, Debug, PartialEq)]
pub struct CompactProgressModel {
    pub stage: String,
    pub current: Option<u32>,
    pub total: Option<u32>,
}

impl CompactProgressModel {
    /// 计算进度 ratio（0.0–1.0）。
    ///
    /// | 阶段 | ratio |
    /// |---|---|
    /// | preparing | 0.05 |
    /// | summarizing（单次） | 0.50 |
    /// | summarizing（chunk i/N） | `0.15 + 0.70*(i/N)` |
    /// | finalizing | 0.90 |
    pub fn ratio(&self) -> f64 {
        match self.stage.as_str() {
            "preparing" => 0.05,
            "summarizing" => match (self.current, self.total) {
                (Some(i), Some(n)) if n > 0 => 0.15 + 0.70 * (i as f64 / n as f64),
                _ => 0.50,
            },
            "finalizing" => 0.90,
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ratio_preparing() {
        let m = CompactProgressModel {
            stage: "preparing".into(),
            current: None,
            total: None,
        };
        assert!((m.ratio() - 0.05).abs() < 1e-9);
    }

    #[test]
    fn test_ratio_summarizing_single() {
        let m = CompactProgressModel {
            stage: "summarizing".into(),
            current: None,
            total: None,
        };
        assert!((m.ratio() - 0.50).abs() < 1e-9);
    }

    #[test]
    fn test_ratio_summarizing_chunk() {
        let m = CompactProgressModel {
            stage: "summarizing".into(),
            current: Some(2),
            total: Some(4),
        };
        assert!((m.ratio() - 0.50).abs() < 1e-9);
    }

    #[test]
    fn test_ratio_finalizing() {
        let m = CompactProgressModel {
            stage: "finalizing".into(),
            current: None,
            total: None,
        };
        assert!((m.ratio() - 0.90).abs() < 1e-9);
    }

    #[test]
    fn test_ratio_summarizing_first_chunk() {
        let m = CompactProgressModel {
            stage: "summarizing".into(),
            current: Some(1),
            total: Some(3),
        };
        // 0.15 + 0.70 * (1/3) ≈ 0.3833
        let expected = 0.15 + 0.70 * (1.0 / 3.0);
        assert!((m.ratio() - expected).abs() < 1e-6);
    }
}
