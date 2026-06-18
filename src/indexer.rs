use crate::symbol_table::{self, SymbolTable};
use std::path::PathBuf;
use tree_sitter::{Parser, Tree};
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

fn collect_class_info(tree: Tree, source: &String) {
    let mut info: SymbolTable;
}
