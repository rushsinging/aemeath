#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = HookApiMarker;
        assert_eq!(marker, marker);
    }
}
