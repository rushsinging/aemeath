#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = PolicyApiMarker;
        assert_eq!(marker, marker);
    }
}
