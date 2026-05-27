#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = StorageApiMarker;
        assert_eq!(marker, marker);
    }
}
