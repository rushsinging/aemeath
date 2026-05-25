#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = ProjectApiMarker;
        assert_eq!(marker, marker);
    }
}
