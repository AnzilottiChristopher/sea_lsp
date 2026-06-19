mod indexer;
mod symbol_table;

use symbol_table::SymbolTable;

fn main() {
    let mut symbol_table = SymbolTable::default();
    indexer::index_workspace(&mut symbol_table);

    // print everything found
    for (name, class) in &symbol_table.classes {
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
