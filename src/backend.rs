use crate::indexer;
use crate::symbol_table::SymbolTable;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, async_trait};

pub struct Backend {
    pub client: Client,
    pub symbol_table: Arc<RwLock<SymbolTable>>,
    pub workspace_root: Arc<RwLock<Option<PathBuf>>>,
    pub documents: Arc<RwLock<HashMap<Url, String>>>,
}

#[async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let workspace_root = params
            .root_uri
            .and_then(|uri| uri.to_file_path().ok())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        if let Ok(mut root) = self.workspace_root.write() {
            *root = Some(workspace_root);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        if let Ok(root) = self.workspace_root.read() {
            if let Some(path) = root.as_ref() {
                if let Ok(mut st) = self.symbol_table.write() {
                    indexer::index_workspace(&mut st, path);
                }
            }
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;

        if let Ok(mut docs) = self.documents.write() {
            docs.insert(uri.clone(), text);
        }

        if let Ok(path) = uri.to_file_path() {
            if let Ok(mut st) = self.symbol_table.write() {
                st.remove_file(&path);
                indexer::index_file(&path, &mut st);
            }
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        if let Some(change) = params.content_changes.into_iter().last() {
            if let Ok(mut docs) = self.documents.write() {
                docs.insert(uri.clone(), change.text);
            }
        }

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

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri;
        let position = pos.position;

        let source = match self
            .documents
            .read()
            .ok()
            .and_then(|d| d.get(&uri).cloned())
        {
            Some(s) => s.replace("\r\n", "\n"),
            None => match uri
                .to_file_path()
                .ok()
                .and_then(|p| std::fs::read_to_string(p).ok())
            {
                Some(s) => s.replace("\r\n", "\n"),
                None => return Ok(None),
            },
        };

        let word =
            get_word_at_position(&source, position.line as usize, position.character as usize);

        if let Ok(st) = self.symbol_table.read() {
            if let Some(class) = st.classes.get(&word) {
                let target_uri = match Url::from_file_path(&class.file) {
                    Ok(uri) => uri,
                    Err(_) => return Ok(None),
                };
                let target_range = Range {
                    start: Position {
                        line: (class.line - 1) as u32,
                        character: 0,
                    },
                    end: Position {
                        line: (class.line - 1) as u32,
                        character: 0,
                    },
                };

                return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                    uri: target_uri,
                    range: target_range,
                })));
            }
        }

        Ok(None)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri;
        let position = pos.position;

        let source = match self
            .documents
            .read()
            .ok()
            .and_then(|d| d.get(&uri).cloned())
        {
            Some(s) => s.replace("\r\n", "\n"),
            None => match uri
                .to_file_path()
                .ok()
                .and_then(|p| std::fs::read_to_string(p).ok())
            {
                Some(s) => s.replace("\r\n", "\n"),
                None => return Ok(None),
            },
        };

        let word =
            get_word_at_position(&source, position.line as usize, position.character as usize);

        if let Ok(st) = self.symbol_table.read() {
            if let Some(class) = st.classes.get(&word) {
                let mut content = format!("**class {}**", class.name);

                if let Some(parent) = &class.parent {
                    content.push_str(&format!("\n\ninherits: `{}`", parent));
                }

                if !class.implements.is_empty() {
                    content.push_str(&format!(
                        "\n\nimplements: `{}`",
                        class.implements.join(", ")
                    ));
                }

                if !class.fields.is_empty() {
                    content.push_str("\n\n**Fields:**");
                    for field in &class.fields {
                        let vis = if field.is_pub { "pub " } else { "" };
                        content
                            .push_str(&format!("\n- `{}{} {}`", vis, field.type_name, field.name));
                    }
                }

                if !class.methods.is_empty() {
                    content.push_str("\n\n**Methods:**");
                    for method in &class.methods {
                        if !method.is_drop {
                            let vis = if method.is_pub { "pub " } else { "" };
                            content.push_str(&format!("\n- `{}{}`", vis, method.name));
                        }
                    }
                }

                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: content,
                    }),
                    range: None,
                }));
            }
        }

        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        eprintln!("[INFO] completion called");

        let trigger = params
            .context
            .as_ref()
            .and_then(|c| c.trigger_character.as_deref());

        match trigger {
            Some(".") => self.dot_completion(params).await,
            _ => self.static_completion().await,
        }
    }
}

fn get_word_at_position(source: &str, line: usize, character: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if line >= lines.len() {
        return String::new();
    }
    let line_text = lines[line];
    let chars: Vec<char> = line_text.chars().collect();
    if character >= chars.len() {
        return String::new();
    }

    let mut start = character;
    let mut end = character;

    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
        end += 1;
    }

    chars[start..end].iter().collect()
}

impl Backend {
    async fn dot_completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let source = match self
            .documents
            .read()
            .ok()
            .and_then(|d| d.get(&uri).cloned())
        {
            Some(s) => s.replace("\r\n", "\n"),
            None => {
                eprintln!("dot_completion: document not found in cache");
                return Ok(None);
            }
        };

        let var_name =
            get_word_before_dot(&source, position.line as usize, position.character as usize);

        eprintln!(
            "line={} char={} var_name='{}'",
            position.line, position.character, var_name
        );

        if var_name.is_empty() {
            eprintln!("var_name is empty — returning None");
            return Ok(None);
        }

        let type_name = match find_variable_type(&source, &var_name) {
            Some(t) => t,
            None => {
                eprintln!("no type found for '{}'", var_name);
                return Ok(None);
            }
        };

        eprintln!("type_name='{}'", type_name);

        if let Ok(st) = self.symbol_table.read() {
            if let Some(class) = st.classes.get(&type_name) {
                let mut items: Vec<CompletionItem> = Vec::new();

                for method in &class.methods {
                    if method.is_pub && !method.is_drop && !method.is_constructor {
                        let params_str = method
                            .params
                            .iter()
                            .map(|(t, n)| format!("{} {}", t, n))
                            .collect::<Vec<_>>()
                            .join(", ");
                        items.push(CompletionItem {
                            label: method.name.clone(),
                            kind: Some(CompletionItemKind::METHOD),
                            detail: Some(format!(
                                "{}({}) -> {}",
                                method.name, params_str, method.return_type
                            )),
                            ..Default::default()
                        });
                    }
                }

                for field in &class.fields {
                    if field.is_pub {
                        items.push(CompletionItem {
                            label: field.name.clone(),
                            kind: Some(CompletionItemKind::FIELD),
                            detail: Some(format!("{} {}", field.type_name, field.name)),
                            ..Default::default()
                        });
                    }
                }

                eprintln!("returning {} items for class '{}'", items.len(), type_name);
                return Ok(Some(CompletionResponse::Array(items)));
            } else {
                eprintln!("class '{}' not found in symbol table", type_name);
            }
        }

        Ok(None)
    }

    async fn static_completion(&self) -> Result<Option<CompletionResponse>> {
        let mut items: Vec<CompletionItem> = Vec::new();

        for s in &["int", "char", "float", "double", "void", "String"] {
            items.push(CompletionItem {
                label: s.to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                ..Default::default()
            });
        }

        for s in &[
            "class",
            "interface",
            "inherit",
            "implements",
            "pub",
            "new",
            "this",
            "init",
            "import",
            "from",
        ] {
            items.push(CompletionItem {
                label: s.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }

        for s in &[
            "if", "else", "while", "for", "switch", "case", "break", "continue", "return",
        ] {
            items.push(CompletionItem {
                label: s.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }

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

fn get_word_before_dot(source: &str, line: usize, character: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if line >= lines.len() {
        return String::new();
    }
    let line_text = lines[line];
    let chars: Vec<char> = line_text.chars().collect();

    let dot_pos = if character > 0 {
        character - 1
    } else {
        return String::new();
    };

    if dot_pos >= chars.len() || chars[dot_pos] != '.' {
        return String::new();
    }

    let end = dot_pos;
    let mut start = end;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }

    chars[start..end].iter().collect()
}

fn find_variable_type(source: &str, var_name: &str) -> Option<String> {
    for line in source.lines() {
        let trimmed = line.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 {
            let potential_type = parts[0];
            let potential_var = parts[1].trim_end_matches(|c| c == ';' || c == '=');
            if potential_var == var_name {
                return Some(potential_type.to_string());
            }
        }
    }
    None
}
