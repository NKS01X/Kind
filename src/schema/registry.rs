use std::collections::HashMap;
use serde_json::Value;

use super::ast::{SchemaNode, DataType, TypeDefinition, EnumDefinition};
use super::parser::Parser;

#[derive(Debug, Clone, Default)]
pub struct SchemaRegistry {
    pub types: HashMap<String, TypeDefinition>,
    pub enums: HashMap<String, EnumDefinition>,
}

#[derive(Debug, PartialEq)]
pub enum ValidationError {
    TypeNotFound(String),
    FieldMissing(String),
    InvalidType { field: String, expected: String },
    InvalidEnumVariant { field: String, variant: String },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ValidationError::TypeNotFound(t) => write!(f, "Type not found: {}", t),
            ValidationError::FieldMissing(field) => write!(f, "Missing required field: {}", field),
            ValidationError::InvalidType { field, expected } => write!(f, "Invalid type for field '{}', expected {}", field, expected),
            ValidationError::InvalidEnumVariant { field, variant } => write!(f, "Invalid enum variant '{}' for field '{}'", variant, field),
        }
    }
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_schema(&mut self, sdl: &str) -> Result<(), String> {
        let mut parser = Parser::new(sdl);
        let nodes = parser.parse()?;
        for node in nodes {
            match node {
                SchemaNode::Type(t) => {
                    self.types.insert(t.name.clone(), t);
                }
                SchemaNode::Enum(e) => {
                    self.enums.insert(e.name.clone(), e);
                }
            }
        }
        Ok(())
    }

    pub fn validate(&self, type_name: &str, data: &Value) -> Result<(), ValidationError> {
        let t_def = self.types.get(type_name).ok_or_else(|| ValidationError::TypeNotFound(type_name.to_string()))?;

        let obj = data.as_object().ok_or_else(|| ValidationError::InvalidType { field: "root".to_string(), expected: "Object".to_string() })?;

        for field_def in &t_def.fields {
            let val = obj.get(&field_def.name).ok_or_else(|| ValidationError::FieldMissing(field_def.name.clone()))?;
            self.validate_field(&field_def.name, &field_def.data_type, val)?;
        }

        Ok(())
    }

    fn validate_field(&self, field_name: &str, data_type: &DataType, val: &Value) -> Result<(), ValidationError> {
        match data_type {
            DataType::String => {
                if !val.is_string() {
                    return Err(ValidationError::InvalidType { field: field_name.to_string(), expected: "String".to_string() });
                }
            }
            DataType::U16 | DataType::U32 | DataType::U64 | DataType::I32 | DataType::I64 => {
                if !val.is_i64() && !val.is_u64() {
                    return Err(ValidationError::InvalidType { field: field_name.to_string(), expected: "Integer".to_string() });
                }
            }
            DataType::F64 => {
                if !val.is_f64() && !val.is_i64() && !val.is_u64() {
                    return Err(ValidationError::InvalidType { field: field_name.to_string(), expected: "Float".to_string() });
                }
            }
            DataType::Boolean => {
                if !val.is_boolean() {
                    return Err(ValidationError::InvalidType { field: field_name.to_string(), expected: "Boolean".to_string() });
                }
            }
            DataType::Custom(custom_type) => {
                if let Some(enum_def) = self.enums.get(custom_type) {
                    if let Some(s) = val.as_str() {
                        if !enum_def.variants.contains(&s.to_string()) {
                            return Err(ValidationError::InvalidEnumVariant { field: field_name.to_string(), variant: s.to_string() });
                        }
                    } else {
                        return Err(ValidationError::InvalidType { field: field_name.to_string(), expected: "String (Enum Variant)".to_string() });
                    }
                } else if let Some(_) = self.types.get(custom_type) {
                    self.validate(custom_type, val)?;
                } else {
                    return Err(ValidationError::TypeNotFound(custom_type.clone()));
                }
            }
        }
        Ok(())
    }
}
