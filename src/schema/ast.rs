#[derive(Debug, Clone, PartialEq)]
pub enum DataType {
    String,
    U16,
    U32,
    U64,
    I32,
    I64,
    F64,
    Boolean,
    Custom(String), // Reference to an enum or another struct
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDefinition {
    pub name: String,
    pub data_type: DataType,
    pub is_indexed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDefinition {
    pub name: String,
    pub fields: Vec<FieldDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDefinition {
    pub name: String,
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaNode {
    Type(TypeDefinition),
    Enum(EnumDefinition),
}
