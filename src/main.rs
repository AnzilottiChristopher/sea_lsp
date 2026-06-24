mod backend;
mod indexer;
mod symbol_table;

use backend::Backend;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use symbol_table::SymbolTable;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let symbol_table = Arc::new(RwLock::new(SymbolTable::default()));
    let workspace_root = Arc::new(RwLock::new(None));
    let documents = Arc::new(RwLock::new(HashMap::new()));

    let (service, socket) = LspService::new(|client| Backend {
        client,
        symbol_table: symbol_table.clone(),
        workspace_root: workspace_root.clone(),
        documents: documents.clone(),
    });

    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
