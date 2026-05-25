#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = AuditApiMarker;
        assert_eq!(marker, marker);
    }
}
