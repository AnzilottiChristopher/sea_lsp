mod backend;
mod indexer;
mod symbol_table;

use backend::Backend;
use std::sync::{Arc, RwLock};
use symbol_table::SymbolTable;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let symbol_table = Arc::new(RwLock::new(SymbolTable::default()));

    let (service, socket) = LspService::new(|client| Backend {
        client,
        symbol_table: symbol_table.clone(),
    });

    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
