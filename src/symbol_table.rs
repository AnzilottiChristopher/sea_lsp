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
    pub file_classes: HashMap<PathBuf, Vec<String>>,
}

impl Default for SymbolTable {
    fn default() -> Self {
        SymbolTable {
            classes: HashMap::new(),
            file_classes: HashMap::new(),
        }
    }
}
impl SymbolTable {
    pub fn insert_class(&mut self, class_info: ClassInfo) {
        // track file → classes mapping
        self.file_classes
            .entry(class_info.file.clone())
            .or_default()
            .push(class_info.name.clone());

        // insert into main table
        self.classes.insert(class_info.name.clone(), class_info);
    }
    pub fn remove_file(&mut self, file: &PathBuf) {
        // look up which classes came from this file
        if let Some(class_names) = self.file_classes.remove(file) {
            // remove each one from the symbol table
            for name in class_names {
                self.classes.remove(&name);
            }
        }
    }
    pub fn print_all(&self) {
        for (name, class) in &self.classes {
            println!("Class: {} ({}:{})", name, class.file.display(), class.line);

            if let Some(parent) = &class.parent {
                println!("  inherits: {}", parent);
            }

            if !class.implements.is_empty() {
                println!("  implements: {}", class.implements.join(", "));
            }

            println!("  fields:");
            for field in &class.fields {
                println!(
                    "    {} {} (pub: {}, line: {})",
                    field.type_name, field.name, field.is_pub, field.line
                );
            }

            println!("  methods:");
            for method in &class.methods {
                println!(
                    "    {} (pub: {}, constructor: {}, drop: {}, line: {})",
                    method.name, method.is_pub, method.is_constructor, method.is_drop, method.line
                );
            }

            println!();
        }
    }
}
