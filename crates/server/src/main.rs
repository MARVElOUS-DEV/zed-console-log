#![forbid(unsafe_code)]

mod cleanup;
mod ordered;

use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tower_lsp_server::{
    Client, LanguageServer, LspService, Server,
    jsonrpc::{Error, Result},
    ls_types::*,
};
use zed_console_log_core::{Settings, text};

#[derive(Clone)]
struct Document {
    text: String,
    language: String,
    version: i32,
}

#[derive(Clone, Default)]
struct State {
    settings: Settings,
    documents: HashMap<Uri, Document>,
    invalid_documents: HashSet<Uri>,
    roots: HashMap<Uri, PathBuf>,
    can_apply_edit: bool,
    cleanup_running: bool,
    revision: u64,
    active: bool,
}

struct Backend {
    client: Client,
    state: Arc<Mutex<State>>,
    queue: Arc<ordered::Queue>,
}

fn core_position(position: Position) -> text::Position {
    text::Position {
        line: position.line,
        character: position.character,
    }
}

fn lsp_position(position: text::Position) -> Position {
    Position::new(position.line, position.character)
}

fn settings_value(value: Value) -> Value {
    value
        .get("zed-console-log")
        .or_else(|| value.get("zedConsoleLog"))
        .cloned()
        .unwrap_or(value)
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let settings = Settings::parse(settings_value(
            params.initialization_options.unwrap_or(Value::Null),
        ))
        .map_err(Error::invalid_params)?;
        {
            let mut state = self.state.lock().expect("state poisoned");
            state.settings = settings;
            state.active = true;
            state.can_apply_edit =
                params
                    .capabilities
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| {
                        workspace.apply_edit == Some(true)
                            && workspace
                                .workspace_edit
                                .as_ref()
                                .is_some_and(|edit| edit.document_changes == Some(true))
                    });
            #[allow(deprecated)]
            let folders = params
                .workspace_folders
                .map(|folders| folders.into_iter().map(|folder| folder.uri).collect())
                .unwrap_or_else(|| params.root_uri.into_iter().collect::<Vec<_>>());
            for uri in folders {
                if let Some(path) = cleanup::root_path(&uri) {
                    state.roots.insert(uri, path);
                }
            }
        }
        Ok(InitializeResult {
            offset_encoding: None,
            server_info: Some(ServerInfo {
                name: "zed-console-log".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        ..Default::default()
                    },
                )),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::new("refactor.rewrite.zedConsoleLog.insert"),
                            CodeActionKind::new("source.zedConsoleLog.comment"),
                            CodeActionKind::new("source.zedConsoleLog.uncomment"),
                            CodeActionKind::new("source.zedConsoleLog.delete"),
                            CodeActionKind::new("source.zedConsoleLog.correct"),
                            CodeActionKind::new(cleanup::KIND),
                        ]),
                        resolve_provider: Some(false),
                        ..Default::default()
                    },
                )),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![cleanup::COMMAND.into()],
                    ..Default::default()
                }),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
    }

    async fn shutdown(&self) -> Result<()> {
        let mut state = self.state.lock().expect("state poisoned");
        state.active = false;
        state.revision += 1;
        state.documents.clear();
        state.invalid_documents.clear();
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        let mut state = self.state.lock().expect("state poisoned");
        state.revision += 1;
        state.invalid_documents.remove(&document.uri);
        state.documents.insert(
            document.uri,
            Document {
                text: document.text,
                language: document.language_id,
                version: document.version,
            },
        );
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let error = {
            let mut state = self.state.lock().expect("state poisoned");
            let uri = params.text_document.uri;
            let Some(mut document) = state.documents.get(&uri).cloned() else {
                return;
            };
            if params.text_document.version <= document.version {
                return;
            }
            let mut invalid = false;
            for change in params.content_changes {
                if let Some(range) = change.range {
                    let start = text::offset(&document.text, core_position(range.start));
                    let end = text::offset(&document.text, core_position(range.end));
                    match (start, end) {
                        (Some(start), Some(end)) if start <= end => {
                            document.text.replace_range(start..end, &change.text)
                        }
                        _ => {
                            invalid = true;
                            break;
                        }
                    }
                } else {
                    document.text = change.text;
                }
            }
            state.revision += 1;
            if invalid {
                // Do not offer edits against a stale snapshot after a synchronization failure.
                state.documents.remove(&uri);
                state.invalid_documents.insert(uri);
                Some("Invalid document change; close and reopen the buffer to resynchronize")
            } else {
                document.version = params.text_document.version;
                state.documents.insert(uri, document);
                None
            }
        };
        if let Some(message) = error {
            self.client.log_message(MessageType::WARNING, message).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let mut state = self.state.lock().expect("state poisoned");
        state.revision += 1;
        state.documents.remove(&params.text_document.uri);
        state.invalid_documents.remove(&params.text_document.uri);
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        match Settings::parse(settings_value(params.settings)) {
            Ok(settings) => self.state.lock().expect("state poisoned").settings = settings,
            Err(message) => self
                .client
                .log_message(
                    MessageType::ERROR,
                    format!(
                        "Invalid zed-console-log settings; previous settings retained: {message}"
                    ),
                )
                .await,
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        let mut state = self.state.lock().expect("state poisoned");
        state.revision += 1;
        for folder in params.event.removed {
            state.roots.remove(&folder.uri);
        }
        for folder in params.event.added {
            if let Some(path) = cleanup::root_path(&folder.uri) {
                state.roots.insert(folder.uri, path);
            }
        }
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        if params.command != cleanup::COMMAND || !params.arguments.is_empty() {
            return Err(Error::invalid_params(
                "Unknown command or unexpected arguments",
            ));
        }
        let snapshot = {
            let mut state = self.state.lock().expect("state poisoned");
            if state.roots.is_empty() || !state.can_apply_edit {
                return Err(Error::invalid_params(
                    "Workspace cleanup requires a local workspace and versioned workspace-edit support",
                ));
            }
            if state.cleanup_running {
                return Err(Error::invalid_params(
                    "A workspace cleanup is already in progress",
                ));
            }
            state.cleanup_running = true;
            state.clone()
        };
        // Do not block the ordered LSP loop while the user reviews the preview.
        cleanup::start(
            self.client.clone(),
            self.state.clone(),
            snapshot,
            self.queue.clone(),
        );
        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let (document, settings, workspace_cleanup) = {
            let state = self.state.lock().expect("state poisoned");
            let Some(document) = state.documents.get(&params.text_document.uri) else {
                return Ok(Some(Vec::new()));
            };
            let workspace_cleanup = state.can_apply_edit
                && cleanup::contains_document(&state.roots, &params.text_document.uri)
                && zed_console_log_core::actions::source_type(&document.language).is_some();
            (document.clone(), state.settings.clone(), workspace_cleanup)
        };
        let start = text::offset(&document.text, core_position(params.range.start))
            .ok_or_else(|| Error::invalid_params("Invalid start position"))?;
        let end = text::offset(&document.text, core_position(params.range.end))
            .ok_or_else(|| Error::invalid_params("Invalid end position"))?;
        if start > end {
            return Err(Error::invalid_params("Reversed selection"));
        }
        let only: Vec<_> = params
            .context
            .only
            .unwrap_or_default()
            .iter()
            .map(|k| k.as_str().to_owned())
            .collect();
        let actions = zed_console_log_core::code_actions(
            params.text_document.uri.as_str(),
            &document.language,
            &document.text,
            start..end,
            &settings,
            &only,
        );
        let mut result = Vec::new();
        for action in actions {
            let mut edits = Vec::new();
            for edit in action.edits {
                let start =
                    text::position(&document.text, edit.start).ok_or_else(Error::internal_error)?;
                let end =
                    text::position(&document.text, edit.end).ok_or_else(Error::internal_error)?;
                edits.push(OneOf::Left(TextEdit {
                    range: Range::new(lsp_position(start), lsp_position(end)),
                    new_text: edit.text,
                }));
            }
            result.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: action.title,
                kind: Some(CodeActionKind::from(action.kind)),
                is_preferred: Some(action.preferred),
                edit: Some(WorkspaceEdit {
                    document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier {
                            uri: params.text_document.uri.clone(),
                            version: Some(document.version),
                        },
                        edits,
                    }])),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
        if workspace_cleanup && start == end && cleanup::matches_kind(&only) {
            result.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: cleanup::TITLE.into(),
                kind: Some(CodeActionKind::new(cleanup::KIND)),
                is_preferred: Some(false),
                command: Some(Command {
                    title: cleanup::TITLE.into(),
                    command: cleanup::COMMAND.into(),
                    arguments: None,
                }),
                ..Default::default()
            }));
        }
        Ok(Some(result))
    }
}

#[tokio::main]
async fn main() {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [] => {}
        [argument] if argument == "--stdio" => {}
        [argument] if argument == "--version" => {
            println!("zed-console-log {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        [argument] if argument == "--help" || argument == "-h" => {
            println!("zed-console-log [--stdio | --version | --help]");
            return;
        }
        _ => {
            eprintln!("Unknown arguments. Usage: zed-console-log [--stdio | --version | --help]");
            std::process::exit(2);
        }
    }
    let queue = Arc::new(ordered::Queue::default());
    let (service, socket) = LspService::new(|client| Backend {
        client,
        state: Arc::new(Mutex::new(State::default())),
        queue: queue.clone(),
    });
    // Preserve notification/request ordering, including incremental edits before code actions.
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .concurrency_level(1)
        .serve(ordered::Ordered {
            inner: service,
            queue,
        })
        .await;
}
