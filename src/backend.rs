use crate::indexer;
use crate::symbol_table::SymbolTable;
use std::sync::{Arc, RwLock};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, async_trait};

pub struct Backend {
    pub client: Client,
    pub symbol_table: Arc<RwLock<SymbolTable>>,
}

#[async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let workspace_root = params
            .root_uri
            .and_then(|uri| uri.to_file_path().ok())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        if let Ok(mut st) = self.symbol_table.write() {
            indexer::index_workspace(&mut st, &workspace_root);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                completion_provider: Some(CompletionOptions::default()),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Ok(path) = uri.to_file_path() {
            if let Ok(mut st) = self.symbol_table.write() {
                st.remove_file(&path);
                indexer::index_file(&path, &mut st);
            }
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Ok(path) = uri.to_file_path() {
            if let Ok(mut st) = self.symbol_table.write() {
                st.remove_file(&path);
                indexer::index_file(&path, &mut st);
            }
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Ok(path) = uri.to_file_path() {
            if let Ok(mut st) = self.symbol_table.write() {
                st.remove_file(&path);
                indexer::index_file(&path, &mut st);
            }
        }
    }

    async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
        let mut items: Vec<CompletionItem> = Vec::new();

        // C primitives and Sea types → TypeParameter kind
        for s in &["int", "char", "float", "double", "void", "String"] {
            items.push(CompletionItem {
                label: s.to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                ..Default::default()
            });
        }

        // Sea keywords
        for s in &[
            "class",
            "interface",
            "inherit",
            "implements",
            "pub",
            "new",
            "this",
            "init",
            "drop",
            "import",
            "from",
        ] {
            items.push(CompletionItem {
                label: s.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }

        // C keywords
        for s in &[
            "if", "else", "while", "for", "switch", "case", "break", "continue", "return",
        ] {
            items.push(CompletionItem {
                label: s.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }

        // Class names from symbol symbol_table
        if let Ok(st) = self.symbol_table.read() {
            for (name, _) in &st.classes {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    ..Default::default()
                });
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }
}
