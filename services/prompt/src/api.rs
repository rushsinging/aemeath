pub use crate::guidance;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = PromptApiMarker;
        assert_eq!(marker, marker);
    }
}
