pub mod ast;
pub mod parser;
pub mod registry;

pub use ast::*;
pub use parser::*;
pub use registry::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_ksl_parsing_and_validation() {
        let sdl = r#"
            enum ContainerStatus {
                Running, Unhealthy, Draining, Stopped
            }

            type ContainerRecord {
                id: String,
                port: U16,
                status: ContainerStatus
            }
        "#;

        let mut registry = SchemaRegistry::new();
        registry.load_schema(sdl).expect("Failed to load schema");

        assert!(registry.types.contains_key("ContainerRecord"));
        assert!(registry.enums.contains_key("ContainerStatus"));

        // Test valid JSON
        let valid_json = json!({
            "id": "abc-123",
            "port": 8080,
            "status": "Running"
        });
        assert_eq!(registry.validate("ContainerRecord", &valid_json), Ok(()));

        // Test invalid enum variant
        let invalid_enum = json!({
            "id": "abc-123",
            "port": 8080,
            "status": "Crashed" // invalid
        });
        assert!(matches!(
            registry.validate("ContainerRecord", &invalid_enum),
            Err(ValidationError::InvalidEnumVariant { .. })
        ));

        // Test missing field
        let missing_field = json!({
            "id": "abc-123",
            "status": "Running"
        });
        assert!(matches!(
            registry.validate("ContainerRecord", &missing_field),
            Err(ValidationError::FieldMissing(_))
        ));

        // Test wrong type
        let wrong_type = json!({
            "id": "abc-123",
            "port": "8080", // string instead of U16
            "status": "Running"
        });
        assert!(matches!(
            registry.validate("ContainerRecord", &wrong_type),
            Err(ValidationError::InvalidType { .. })
        ));
    }

    #[test]
    fn test_ksl_array_support() {
        let sdl = r#"
            type User {
                id: String,
                friends: [String],
                scores: [[U32]]
            }
        "#;

        let mut registry = SchemaRegistry::new();
        registry.load_schema(sdl).expect("Failed to load schema");

        assert!(registry.types.contains_key("User"));

        // Valid JSON
        let valid_json = json!({
            "id": "u1",
            "friends": ["alice", "bob"],
            "scores": [[10, 20], [30]]
        });
        assert_eq!(registry.validate("User", &valid_json), Ok(()));

        // Invalid inner type
        let invalid_inner = json!({
            "id": "u1",
            "friends": ["alice", 123],
            "scores": [[10, 20], [30]]
        });
        assert!(matches!(
            registry.validate("User", &invalid_inner),
            Err(ValidationError::InvalidType { .. })
        ));

        // Invalid structure (not an array)
        let invalid_struct = json!({
            "id": "u1",
            "friends": "alice",
            "scores": [[10, 20], [30]]
        });
        assert!(matches!(
            registry.validate("User", &invalid_struct),
            Err(ValidationError::InvalidType { .. })
        ));
    }
}
