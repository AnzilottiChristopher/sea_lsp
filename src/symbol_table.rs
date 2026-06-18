use std::collections::HashMap;
use std::path::PathBuf;

pub struct ClassInfo {
    name: String,
    fields: Vec<FieldInfo>,   // pub fields only
    methods: Vec<MethodInfo>, // pub methods only
    parent: Option<String>,
    implements: Vec<String>,
    file: PathBuf,
    line: usize,
}

pub struct FieldInfo {
    name: String,
    type_name: String,
    line: usize,
}

pub struct MethodInfo {
    name: String,
    params: Vec<(String, String)>, // (type, name)
    return_type: String,
    line: usize,
}

pub struct SymbolTable {
    classes: HashMap<String, ClassInfo>,
}
