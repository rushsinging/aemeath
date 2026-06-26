/// Compact 进度模型。
///
/// 由 runtime 的 `CompactProgress` 事件驱动，TUI 据此渲染 Gauge 进度条。
#[derive(Clone, Debug, PartialEq)]
pub struct CompactProgressModel {
    pub stage: String,
    pub current: Option<u32>,
    pub total: Option<u32>,
}

impl CompactProgressModel {
    /// 计算 Gauge ratio（0.0–1.0）。
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

    /// 计算 Gauge label。
    pub fn label(&self) -> String {
        match self.stage.as_str() {
            "preparing" => "Compacting — preparing...".to_string(),
            "summarizing" => match (self.current, self.total) {
                (Some(i), Some(n)) if n > 0 => {
                    format!("Compacting — summarizing (chunk {i}/{n})")
                }
                _ => "Compacting — summarizing...".to_string(),
            },
            "finalizing" => "Compacting — finalizing...".to_string(),
            _ => "Compacting...".to_string(),
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
        assert_eq!(m.label(), "Compacting — preparing...");
    }

    #[test]
    fn test_ratio_summarizing_single() {
        let m = CompactProgressModel {
            stage: "summarizing".into(),
            current: None,
            total: None,
        };
        assert!((m.ratio() - 0.50).abs() < 1e-9);
        assert_eq!(m.label(), "Compacting — summarizing...");
    }

    #[test]
    fn test_ratio_summarizing_chunk() {
        let m = CompactProgressModel {
            stage: "summarizing".into(),
            current: Some(2),
            total: Some(4),
        };
        assert!((m.ratio() - 0.50).abs() < 1e-9);
        assert_eq!(m.label(), "Compacting — summarizing (chunk 2/4)");
    }

    #[test]
    fn test_ratio_finalizing() {
        let m = CompactProgressModel {
            stage: "finalizing".into(),
            current: None,
            total: None,
        };
        assert!((m.ratio() - 0.90).abs() < 1e-9);
        assert_eq!(m.label(), "Compacting — finalizing...");
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
