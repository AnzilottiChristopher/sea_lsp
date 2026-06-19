use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Default)]
pub struct ClassInfo {
    pub name: String,
    pub fields: Vec<FieldInfo>,   // pub fields only
    pub methods: Vec<MethodInfo>, // pub methods only
    pub parent: Option<String>,
    pub implements: Vec<String>,
    pub file: PathBuf,
    pub line: usize,
}

pub struct FieldInfo {
    pub name: String,
    pub type_name: String,
    pub line: usize,
    pub is_pub: bool,
}

pub struct MethodInfo {
    pub name: String,
    pub params: Vec<(String, String)>, // (type, name)
    pub return_type: String,
    pub line: usize,
    pub is_pub: bool,
    pub is_constructor: bool,
    pub is_drop: bool, // Should of made the three is_* an enum
}

pub struct SymbolTable {
    pub classes: HashMap<String, ClassInfo>,
}
