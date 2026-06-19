use crate::symbol_table::{ClassInfo, FieldInfo, MethodInfo, SymbolTable};
use std::path::PathBuf;
use tree_sitter::{Node, Parser, Tree};
use walkdir::WalkDir;

fn collect_files() -> Vec<PathBuf> {
    // Maybe add a src check because currently this just checks
    // the current directory the file is in and everything below it's level
    let sea_files: Vec<_> = WalkDir::new(".")
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "sea"))
        .map(|e| e.path().to_path_buf())
        .collect();
    sea_files
}

fn parse_dirs(dir: &Vec<PathBuf>) -> Vec<(Tree, String)> {
    let language: tree_sitter::Language = tree_sitter_sea::LANGUAGE.into();
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("Error loading Sea grammar");

    let mut sources: Vec<(Tree, String)> = Vec::new();
    // For each directory
    // Walk through and collect tree and source
    for path in dir {
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| {
                eprintln!("Error reading {:?}: {}", path, e);
                std::process::exit(1);
            })
            .replace("\r\n", "\n");
        let tree = parser.parse(&source, None).unwrap();
        sources.push((tree, source));
    }
    sources
}

fn collect_info(tree: Tree, source: &String, symbol_table: &mut SymbolTable, file: &PathBuf) {
    let root = tree.root_node();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        match child.kind() {
            "import_declaration" => {
                let path_node = child.child_by_field_name("path").unwrap();
                let path_text = &source[path_node.start_byte()..path_node.end_byte()];
                let path_str = path_text.trim_matches('"');

                let current_dir = file.parent().unwrap_or(std::path::Path::new("."));
                let imported_path = current_dir.join(format!("{}.sea", path_str));

                // only index if file exists and hasn't been indexed yet
                if imported_path.exists() && !symbol_table.file_classes.contains_key(&imported_path)
                {
                    let imported_source = std::fs::read_to_string(&imported_path)
                        .unwrap_or_else(|e| {
                            eprintln!("Error reading {:?}: {}", imported_path, e);
                            String::new()
                        })
                        .replace("\r\n", "\n");

                    let language: tree_sitter::Language = tree_sitter_sea::LANGUAGE.into();
                    let mut parser = Parser::new();
                    parser
                        .set_language(&language)
                        .expect("Error loading Sea grammar");

                    if let Some(imported_tree) = parser.parse(&imported_source, None) {
                        collect_info(
                            imported_tree,
                            &imported_source,
                            symbol_table,
                            &imported_path,
                        );
                    }
                }
            }
            "class_declaration" => {
                // First grab basic class info like name
                // Then walk into class for pub fields and methods
                let mut class_info: ClassInfo = ClassInfo::default();
                collect_class_info(&child, source, &mut class_info);
                class_info.file = file.clone();
                symbol_table.insert_class(class_info);
            }
            _ => {}
        }
    }
}

fn collect_class_info(node: &Node, source: &String, class_info: &mut ClassInfo) {
    let name_node = node.child_by_field_name("name").unwrap();

    //Name and line fields for class info
    let name = source[name_node.start_byte()..name_node.end_byte()].to_string();
    let line = node.start_position().row + 1;

    //inherit and implements if it has
    let inherits = node
        .child_by_field_name("inherit")
        .and_then(|inherit| inherit.child_by_field_name("parent"))
        .map(|p| source[p.start_byte()..p.end_byte()].to_string());

    let mut implements = Vec::new();
    if let Some(implements_node) = node.child_by_field_name("implements") {
        let mut cursor = implements_node.walk();
        for child in implements_node.children(&mut cursor) {
            if child.kind() == "identifier" {
                implements.push(source[child.start_byte()..child.end_byte()].to_string());
            }
        }
    }

    // Now need to walk through the body using for loop
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "field_declaration" => {
                class_info.fields.push(collect_field_info(&child, source));
            }
            "method_declaration" => {
                class_info.methods.push(collect_method_info(&child, source));
            }
            "constructor_declaration" => {
                let con_name_node = child.child_by_field_name("name").unwrap();
                let con_name =
                    source[con_name_node.start_byte()..con_name_node.end_byte()].to_string();
                let mut method = collect_method_info(&child, source);
                method.is_constructor = con_name == class_info.name;
                class_info.methods.push(method);
            }
            "drop_declaration" => {
                class_info.methods.push(MethodInfo {
                    name: "drop".to_string(),
                    params: Vec::new(),
                    return_type: "void".to_string(),
                    line: child.start_position().row + 1,
                    is_pub: child.child_by_field_name("visibility").is_some(),
                    is_constructor: false,
                    is_drop: true,
                });
            }
            _ => {}
        }
    }

    class_info.name = name;
    class_info.parent = inherits;
    class_info.implements = implements;
    class_info.line = line;
}

fn collect_field_info(node: &Node, source: &String) -> FieldInfo {
    let is_pub = node.child_by_field_name("visibility").is_some();

    let type_node = node.child_by_field_name("type").unwrap();
    let base_node = type_node.child_by_field_name("base").unwrap();
    let type_text = source[base_node.start_byte()..base_node.end_byte()].to_string();

    let name_node = node.child_by_field_name("name").unwrap();
    let name = source[name_node.start_byte()..name_node.end_byte()].to_string();

    let line = node.start_position().row + 1;

    FieldInfo {
        name,
        type_name: type_text,
        line,
        is_pub,
    }
}
fn collect_method_info(node: &Node, source: &String) -> MethodInfo {
    // method_declaration wraps sea_style_method or c_style_method
    let method_node = node.child(0).unwrap();

    // check if pub
    let is_pub = method_node.child_by_field_name("visibility").is_some();

    // get name
    let name_node = method_node.child_by_field_name("name").unwrap();
    let name = source[name_node.start_byte()..name_node.end_byte()].to_string();

    // get return type
    let return_type = match method_node.child_by_field_name("return_type") {
        Some(return_node) => match return_node.named_child(0) {
            Some(type_node) => match type_node.child_by_field_name("base") {
                Some(base_node) => source[base_node.start_byte()..base_node.end_byte()].to_string(),
                None => "void".to_string(),
            },
            None => "void".to_string(),
        },
        None => "void".to_string(),
    };

    // get params
    let params = match method_node.child_by_field_name("parameters") {
        Some(params_node) => {
            let mut params = Vec::new();
            let mut cursor = params_node.walk();
            for param in params_node.children(&mut cursor) {
                if param.kind() == "sea_parameter" {
                    let type_node = param.child_by_field_name("type").unwrap();
                    let base_node = type_node.child_by_field_name("base").unwrap();
                    let type_text =
                        source[base_node.start_byte()..base_node.end_byte()].to_string();
                    let name_node = param.child_by_field_name("name").unwrap();
                    let name_text =
                        source[name_node.start_byte()..name_node.end_byte()].to_string();
                    params.push((type_text, name_text));
                }
            }
            params
        }
        None => Vec::new(),
    };

    // get line
    let line = node.start_position().row + 1;

    MethodInfo {
        name,
        params,
        return_type,
        line,
        is_pub,
        is_constructor: false,
        is_drop: false,
    }
}
