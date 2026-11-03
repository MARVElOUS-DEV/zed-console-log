use super::{Document, State, lsp_position, ordered::Queue};
use ignore::WalkBuilder;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};
use tower_lsp_server::{Client, ls_types::*};
use zed_console_log_core::{Settings, format::MARKER, text};

pub const COMMAND: &str = "zedConsoleLog.deleteWorkspace";
pub const KIND: &str = "source.zedConsoleLog.deleteWorkspace";
pub const TITLE: &str = "Delete generated logs in workspace…";
const CONFIRM: &str = "Delete logs";
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_SCAN_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const EXCLUDED: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "coverage",
    ".next",
    ".nuxt",
    "vendor",
];

fn local_path(uri: &Uri) -> Option<PathBuf> {
    // Url validates file URIs and handles Windows drive/UNC paths and escaping.
    url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

pub fn root_path(uri: &Uri) -> Option<PathBuf> {
    let path = local_path(uri)?.canonicalize().ok()?;
    path.is_dir().then_some(path)
}

fn safe_open_path(roots: &HashMap<Uri, PathBuf>, uri: &Uri) -> Option<PathBuf> {
    let path = local_path(uri)?;
    let canonical = path.canonicalize().ok()?;
    for (root_uri, root) in roots {
        if !canonical.starts_with(root) {
            continue;
        }
        let alias = local_path(root_uri)?;
        let relative = match path
            .strip_prefix(&alias)
            .or_else(|_| path.strip_prefix(root))
        {
            Ok(relative) => relative,
            Err(_) => continue,
        };
        let mut candidate = root.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return None;
            };
            candidate.push(name);
            if fs::symlink_metadata(&candidate)
                .ok()?
                .file_type()
                .is_symlink()
            {
                return None;
            }
        }
        if candidate == canonical {
            return Some(canonical);
        }
    }
    None
}

pub fn contains_document(roots: &HashMap<Uri, PathBuf>, uri: &Uri) -> bool {
    safe_open_path(roots, uri).is_some()
}

pub fn matches_kind(only: &[String]) -> bool {
    only.is_empty()
        || only.iter().any(|kind| {
            KIND == kind
                || KIND
                    .strip_prefix(kind)
                    .is_some_and(|tail| tail.starts_with('.'))
        })
}

fn quantity(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

struct FilePlan {
    path: PathBuf,
    original: String,
    edit: TextDocumentEdit,
}

struct Plan {
    files: Vec<FilePlan>,
    revision: u64,
    skipped: usize,
}

impl Plan {
    fn count(&self) -> usize {
        self.files.iter().map(|file| file.edit.edits.len()).sum()
    }

    fn preview(&self) -> String {
        let mut message = format!(
            "Delete {} in {}?\n\n",
            quantity(self.count(), "generated log"),
            quantity(self.files.len(), "file")
        );
        for file in &self.files {
            message.push_str(&format!(
                "{} — {}\n",
                file.path.display(),
                quantity(file.edit.edits.len(), "log")
            ));
        }
        if self.skipped > 0 {
            message.push_str(&format!("\n{} files/entries skipped (unreadable, oversized, unsynchronized, or no safe deletions).\n", self.skipped));
        }
        message
            .push_str("\nEdits are applied through the editor; files are not saved automatically.");
        message
    }

    fn validate(&self, state: &State) -> Result<(), String> {
        if !state.active || state.revision != self.revision {
            return Err(
                "Workspace buffers or folders changed during preview. Run cleanup again.".into(),
            );
        }
        for file in &self.files {
            // Reject symlink replacements and files moved outside the scanned roots.
            if !regular_file(&file.path)
                || file.path.canonicalize().ok().as_ref() != Some(&file.path)
            {
                return Err(format!(
                    "{} changed during preview. Run cleanup again.",
                    file.path.display()
                ));
            }
            if file.edit.text_document.version.is_none()
                && read_source(&file.path).as_deref() != Ok(file.original.as_str())
            {
                return Err(format!(
                    "{} changed on disk during preview. Run cleanup again.",
                    file.path.display()
                ));
            }
        }
        Ok(())
    }

    fn workspace_edit(self) -> WorkspaceEdit {
        WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(
                self.files.into_iter().map(|file| file.edit).collect(),
            )),
            ..Default::default()
        }
    }
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn read_source(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_FILE_BYTES as u64 {
        return Err("File exceeds cleanup size limit".into());
    }
    let mut source = String::new();
    Read::by_ref(&mut file)
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_string(&mut source)
        .map_err(|error| error.to_string())?;
    if source.len() > MAX_FILE_BYTES {
        return Err("File exceeds cleanup size limit".into());
    }
    Ok(source)
}

fn language(path: &Path) -> Option<&'static str> {
    Some(
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "js" | "mjs" | "cjs" => "javascript",
            "jsx" => "javascriptreact",
            "ts" | "mts" | "cts" => "typescript",
            "tsx" => "typescriptreact",
            _ => return None,
        },
    )
}

fn file_plan(
    path: PathBuf,
    uri: Uri,
    source: String,
    language: &str,
    version: Option<i32>,
    settings: &Settings,
) -> Option<FilePlan> {
    let action = zed_console_log_core::code_actions(
        uri.as_str(),
        language,
        &source,
        0..0,
        settings,
        &["source.zedConsoleLog.delete".into()],
    )
    .into_iter()
    .next()?;
    let edits = action
        .edits
        .into_iter()
        .map(|edit| {
            Some(OneOf::Left(TextEdit {
                range: Range::new(
                    lsp_position(text::position(&source, edit.start)?),
                    lsp_position(text::position(&source, edit.end)?),
                ),
                new_text: edit.text,
            }))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(FilePlan {
        path,
        original: source,
        edit: TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier { uri, version },
            edits,
        },
    })
}

fn scan(snapshot: &State) -> Result<Plan, String> {
    let mut paths = BTreeMap::new();
    let mut skipped = 0;
    let mut roots: Vec<_> = snapshot.roots.values().collect();
    roots.sort();
    roots.dedup();
    for root in roots {
        // Ignore rules apply even to non-Git projects. Never follow symlinks.
        let walker = WalkBuilder::new(root)
            .hidden(false)
            .require_git(false)
            .follow_links(false)
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !entry.file_type().is_some_and(|kind| kind.is_dir())
                    || !EXCLUDED.iter().any(|name| entry.file_name() == *name)
            })
            .build();
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && language(entry.path()).is_some()
            {
                let path = entry.into_path();
                paths.insert(path, ());
                if paths.len() > MAX_FILES {
                    return Err("Workspace cleanup exceeds the 20,000-file scan limit. Use a smaller workspace.".into());
                }
            }
        }
    }
    let mut open = HashMap::<PathBuf, (&Uri, &Document)>::new();
    let mut blocked = HashSet::new();
    for (uri, document) in &snapshot.documents {
        if let Some(path) = local_path(uri).and_then(|path| path.canonicalize().ok()) {
            // Symlink aliases and conflicting open aliases cannot safely describe a file.
            if safe_open_path(&snapshot.roots, uri).as_ref() != Some(&path) {
                blocked.insert(path);
                continue;
            }
            if open.insert(path.clone(), (uri, document)).is_some() {
                blocked.insert(path);
            }
        }
    }
    for uri in &snapshot.invalid_documents {
        if let Some(path) = local_path(uri).and_then(|path| path.canonicalize().ok()) {
            blocked.insert(path);
        }
    }
    let mut files = Vec::new();
    let mut scan_bytes = 0;
    for (path, ()) in paths {
        if blocked.contains(&path)
            || !regular_file(&path)
            || path.canonicalize().ok().as_ref() != Some(&path)
        {
            skipped += 1;
            continue;
        }
        let (uri, source, language, version) = if let Some((uri, document)) = open.get(&path) {
            (
                (*uri).clone(),
                document.text.clone(),
                document.language.as_str(),
                Some(document.version),
            )
        } else {
            let source = match read_source(&path) {
                Ok(source) => source,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let Some(uri) = url::Url::from_file_path(&path)
                .ok()
                .and_then(|url| url.as_str().parse().ok())
            else {
                skipped += 1;
                continue;
            };
            (
                uri,
                source,
                language(&path).expect("filtered extension"),
                None,
            )
        };
        if source.len() > MAX_FILE_BYTES {
            skipped += 1;
            continue;
        }
        scan_bytes += source.len();
        if scan_bytes > MAX_SCAN_BYTES {
            return Err(
                "Workspace cleanup exceeds the 64 MiB scan limit. Use a smaller workspace.".into(),
            );
        }
        if !source.contains(MARKER) {
            continue;
        }
        if let Some(file) = file_plan(path, uri, source, language, version, &snapshot.settings) {
            files.push(file);
        } else {
            skipped += 1;
        }
    }
    Ok(Plan {
        files,
        revision: snapshot.revision,
        skipped,
    })
}

struct Running(Arc<Mutex<State>>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.lock().expect("state poisoned").cleanup_running = false;
    }
}

pub fn start(client: Client, state: Arc<Mutex<State>>, snapshot: State, queue: Arc<Queue>) {
    tokio::spawn(async move {
        let _running = Running(state.clone());
        if let Err(message) = run(&client, &state, snapshot, &queue).await
            && state.lock().expect("state poisoned").active
        {
            client.show_message(MessageType::WARNING, message).await;
        }
    });
}

async fn run(
    client: &Client,
    state: &Arc<Mutex<State>>,
    snapshot: State,
    queue: &Queue,
) -> Result<(), String> {
    let plan = tokio::task::spawn_blocking(move || scan(&snapshot))
        .await
        .map_err(|error| error.to_string())??;
    queue.drain().await;
    plan.validate(&state.lock().expect("state poisoned"))?;
    if plan.files.is_empty() {
        client
            .show_message(
                MessageType::INFO,
                format!(
                    "No safely removable generated logs found. {} files/entries skipped.",
                    plan.skipped
                ),
            )
            .await;
        return Ok(());
    }
    let choice = client
        .show_message_request(
            MessageType::INFO,
            plan.preview(),
            Some(vec![
                MessageActionItem {
                    title: "Cancel".into(),
                    properties: HashMap::new(),
                },
                MessageActionItem {
                    title: CONFIRM.into(),
                    properties: HashMap::new(),
                },
            ]),
        )
        .await
        .map_err(|error| error.to_string())?;
    if choice.is_none_or(|choice| choice.title != CONFIRM) {
        client
            .show_message(MessageType::INFO, "Workspace cleanup canceled.")
            .await;
        return Ok(());
    }
    queue.drain().await;
    plan.validate(&state.lock().expect("state poisoned"))?;
    let count = plan.count();
    let files = plan.files.len();
    let response = client
        .apply_edit(plan.workspace_edit())
        .await
        .map_err(|error| error.to_string())?;
    if !response.applied {
        return Err(format!(
            "The editor did not apply workspace cleanup: {}",
            response
                .failure_reason
                .unwrap_or_else(|| "edit rejected".into())
        ));
    }
    client
        .show_message(
            MessageType::INFO,
            format!(
                "Removed {} from {}. Save the edited buffers to persist changes.",
                quantity(count, "generated log"),
                quantity(files, "file")
            ),
        )
        .await;
    Ok(())
}
