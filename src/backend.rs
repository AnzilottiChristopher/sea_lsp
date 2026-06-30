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
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![",".to_string()]),
                    ..Default::default()
                }),
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

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
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
            None => return Ok(None),
        };

        // find the enclosing call's "(" and the function/constructor name before it,
        // plus which parameter index the cursor is currently sitting in (by comma count)
        let (word, active_param) =
            match find_enclosing_call(&source, position.line as usize, position.character as usize)
            {
                Some(result) => result,
                None => return Ok(None),
            };

        if word.is_empty() {
            return Ok(None);
        }

        if let Ok(st) = self.symbol_table.read() {
            // check if it's a constructor — new Dog(
            for (class_name, class_info) in &st.classes {
                if class_name == &word {
                    // find init method
                    if let Some(method) = class_info.methods.iter().find(|m| m.is_constructor) {
                        let params_str = method
                            .params
                            .iter()
                            .map(|(t, n)| format!("{} {}", t, n))
                            .collect::<Vec<_>>()
                            .join(", ");

                        let label = format!("init({})", params_str);
                        let param_count = method.params.len();
                        let clamped_active = clamp_active_param(active_param, param_count);

                        return Ok(Some(SignatureHelp {
                            signatures: vec![SignatureInformation {
                                label,
                                documentation: None,
                                parameters: Some(
                                    method
                                        .params
                                        .iter()
                                        .map(|(t, n)| ParameterInformation {
                                            label: ParameterLabel::Simple(format!("{} {}", t, n)),
                                            documentation: None,
                                        })
                                        .collect(),
                                ),
                                active_parameter: clamped_active,
                            }],
                            active_signature: Some(0),
                            active_parameter: clamped_active,
                        }));
                    }
                }

                // check if it's a method call — dog.bark(
                for method in &class_info.methods {
                    if method.name == word && !method.is_constructor {
                        let params_str = method
                            .params
                            .iter()
                            .map(|(t, n)| format!("{} {}", t, n))
                            .collect::<Vec<_>>()
                            .join(", ");

                        let label =
                            format!("{}({}) -> {}", method.name, params_str, method.return_type);
                        let param_count = method.params.len();
                        let clamped_active = clamp_active_param(active_param, param_count);

                        return Ok(Some(SignatureHelp {
                            signatures: vec![SignatureInformation {
                                label,
                                documentation: None,
                                parameters: Some(
                                    method
                                        .params
                                        .iter()
                                        .map(|(t, n)| ParameterInformation {
                                            label: ParameterLabel::Simple(format!("{} {}", t, n)),
                                            documentation: None,
                                        })
                                        .collect(),
                                ),
                                active_parameter: clamped_active,
                            }],
                            active_signature: Some(0),
                            active_parameter: clamped_active,
                        }));
                    }
                }
            }
        }

        Ok(None)
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

        let uri = params.text_document_position.text_document.uri.clone();
        let line = params.text_document_position.position.line as usize;

        let source = self
            .documents
            .read()
            .ok()
            .and_then(|d| d.get(&uri).cloned())
            .unwrap_or_default();

        let trigger = params
            .context
            .as_ref()
            .and_then(|c| c.trigger_character.as_deref());

        match trigger {
            Some(".") => self.dot_completion(params).await,
            _ => self.static_completion(&source, line).await,
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

        let type_name = if var_name == "this" {
            match find_enclosing_class(&source, position.line as usize) {
                Some(class_name) => class_name,
                None => {
                    eprintln!("this. but no enclosing class found");
                    return Ok(None);
                }
            }
        } else {
            match find_variable_type(&source, &var_name) {
                Some(t) => t,
                None => {
                    eprintln!("no type found for '{}'", var_name);
                    return Ok(None);
                }
            }
        };

        eprintln!("type_name='{}'", type_name);

        if let Ok(st) = self.symbol_table.read() {
            if let Some(class) = st.classes.get(&type_name) {
                let mut items: Vec<CompletionItem> = Vec::new();
                let is_this = var_name == "this";

                for method in &class.methods {
                    if (is_this || method.is_pub) && !method.is_drop && !method.is_constructor {
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
                            insert_text: Some(format!("{}($1)", method.name)),
                            insert_text_format: Some(InsertTextFormat::SNIPPET),
                            ..Default::default()
                        });
                    }
                }

                for field in &class.fields {
                    if is_this || field.is_pub {
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

    async fn static_completion(
        &self,
        source: &str,
        line: usize,
    ) -> Result<Option<CompletionResponse>> {
        let mut items: Vec<CompletionItem> = Vec::new();

        for (var_name, var_type) in collect_local_vars(source, line) {
            items.push(CompletionItem {
                label: var_name,
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(var_type),
                ..Default::default()
            });
        }
        for (param_name, param_type) in collect_params(source, line) {
            items.push(CompletionItem {
                label: param_name,
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(param_type),
                ..Default::default()
            });
        }

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

/// Scans backward across (possibly multiple) lines from `(line, character)` to find
/// the unmatched, enclosing '(' for the cursor's current position, tracking paren
/// depth so nested calls like `foo(bar(1, 2), |)` resolve to `foo`'s paren, not `bar`'s.
///
/// Returns `(callee_name, active_parameter_index)` where the index is the number of
/// depth-0 commas seen between the enclosing '(' and the cursor.
fn find_enclosing_call(source: &str, line: usize, character: usize) -> Option<(String, u32)> {
    let lines: Vec<&str> = source.lines().collect();
    if line >= lines.len() {
        return None;
    }

    let mut depth: i32 = 0;
    let mut comma_count: u32 = 0;

    // Walk backward line by line, starting at the cursor's line/character.
    for cur_line in (0..=line).rev() {
        let chars: Vec<char> = lines[cur_line].chars().collect();
        let start_char = if cur_line == line {
            character.min(chars.len())
        } else {
            chars.len()
        };

        let mut i = start_char;
        while i > 0 {
            i -= 1;
            match chars[i] {
                ')' => depth += 1,
                '(' => {
                    if depth == 0 {
                        // found the enclosing '(' — extract the identifier before it
                        let end = i;
                        let mut start = end;
                        while start > 0
                            && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_')
                        {
                            start -= 1;
                        }
                        let word: String = chars[start..end].iter().collect();
                        return Some((word, comma_count));
                    } else {
                        depth -= 1;
                    }
                }
                ',' if depth == 0 => comma_count += 1,
                _ => {}
            }
        }
    }

    None
}

fn clamp_active_param(active_param: u32, param_count: usize) -> Option<u32> {
    if param_count == 0 {
        return Some(0);
    }
    Some(active_param.min(param_count as u32 - 1))
}

fn find_enclosing_class(source: &str, line: usize) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut brace_depth: i32 = 0;
    let mut found_class: Option<String> = None;
    let mut inside_method = false;

    for i in (0..=line).rev() {
        let trimmed = lines[i].trim();

        for c in trimmed.chars().rev() {
            match c {
                '}' => brace_depth += 1,
                '{' => brace_depth -= 1,
                _ => {}
            }
        }

        if brace_depth < 0 {
            if trimmed.starts_with("class ") {
                // only count if we passed through a method body first
                if inside_method {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() >= 2 {
                        found_class = Some(
                            parts[1]
                                .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_')
                                .to_string(),
                        );
                    }
                }
                brace_depth = 0;
            } else {
                // hit a non-class opener — this is a method body
                inside_method = true;
                brace_depth = 0;
            }
        }
    }

    found_class
}

fn collect_local_vars(source: &str, up_to_line: usize) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for line in source.lines().take(up_to_line + 1) {
        let trimmed = line.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 {
            let potential_type = parts[0];
            let potential_var = parts[1].trim_end_matches(|c| c == ';' || c == '=' || c == '(');
            // skip keywords
            if !["if", "while", "for", "return", "pub", "void", "class"].contains(&potential_type) {
                vars.insert(potential_var.to_string(), potential_type.to_string());
            }
        }
    }
    vars
}

fn collect_params(source: &str, line: usize) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut brace_depth: i32 = 0;

    for i in (0..=line).rev() {
        let trimmed = lines[i].trim();

        // track brace depth walking backward
        for c in trimmed.chars().rev() {
            match c {
                '}' => brace_depth += 1,
                '{' => brace_depth -= 1,
                _ => {}
            }
        }

        // when brace_depth goes negative we've found the opening brace
        // of the scope the cursor is in — this line is the method signature
        if brace_depth < 0 {
            // make sure it's actually a method/init signature, not a class or if/while
            // a method signature will have a '(' and ')' before the '{'
            if let Some(paren_start) = trimmed.find('(') {
                if let Some(paren_end) = trimmed.rfind(')') {
                    // make sure this isn't a class declaration or control flow
                    let before_paren = trimmed[..paren_start].trim();
                    let is_control_flow = ["if", "while", "for", "switch"]
                        .iter()
                        .any(|kw| before_paren.ends_with(kw));
                    let is_class = before_paren.starts_with("class ");

                    if !is_control_flow && !is_class {
                        let param_section = &trimmed[paren_start + 1..paren_end];
                        for param in param_section.split(',') {
                            let parts: Vec<&str> = param.trim().split_whitespace().collect();
                            if parts.len() == 2 {
                                params.insert(parts[1].to_string(), parts[0].to_string());
                            }
                        }
                    }
                }
            }
            break; // whether we found params or not, stop here
        }

        // stop if we've walked out past the class entirely
        if trimmed.starts_with("class ") {
            break;
        }
    }

    params
}
