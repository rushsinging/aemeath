use mongodb::bson::oid::ObjectId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    InvalidObjectId { value: String },
}

pub fn parse_object_id(value: &str) -> Result<ObjectId, IdError> {
    ObjectId::parse_str(value).map_err(|_| IdError::InvalidObjectId {
        value: value.to_string(),
    })
}

pub fn object_id_to_string(id: &ObjectId) -> String {
    id.to_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_object_id_accepts_valid_hex_string() {
        let id = parse_object_id("507f1f77bcf86cd799439011").expect("valid object id");

        assert_eq!(id.to_hex(), "507f1f77bcf86cd799439011");
    }

    #[test]
    fn test_parse_object_id_rejects_non_hex_string() {
        let result = parse_object_id("507f1f77bcf86cd79943901z");

        assert!(
            matches!(result, Err(IdError::InvalidObjectId { value }) if value == "507f1f77bcf86cd79943901z")
        );
    }

    #[test]
    fn test_parse_object_id_rejects_wrong_length_string() {
        let result = parse_object_id("507f1f77bcf86cd7994390");

        assert!(
            matches!(result, Err(IdError::InvalidObjectId { value }) if value == "507f1f77bcf86cd7994390")
        );
    }

    #[test]
    fn test_object_id_to_string_returns_hex_string() {
        let id = ObjectId::parse_str("507f1f77bcf86cd799439011").expect("valid object id");

        assert_eq!(object_id_to_string(&id), "507f1f77bcf86cd799439011");
    }
}
