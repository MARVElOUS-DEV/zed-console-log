# Architecture

The implementation has three Rust packages:

```text
Zed
 ├─ existing JS/TS language servers
 └─ extension/ (WebAssembly launcher)
      └─ zed-console-log --stdio (native process)
           └─ zed-console-log-core (Oxc + edit planning)
```

The native server can also be launched by another LSP editor. The extension forwards settings and selects an explicitly configured executable, one on PATH, or a native GitHub Release asset pinned to the extension's Cargo version. Downloaded binaries live in Zed's extension work directory. A staging directory is promoted to the versioned cache only after extraction, binary validation, and executable permissions succeed; cached binaries are reused without a network request. The extension has no runtime JavaScript bridge and does not bundle the server.

## Documents and protocol

The server stores each open document's language ID, version, and current text. Incremental updates apply sequentially and atomically to a cloned snapshot. Stale versions are ignored; invalid ranges invalidate the snapshot. Per-document actions never read stale disk source for an open buffer. The explicit workspace cleanup command also reads eligible closed files from registered local workspace folders.

LSP positions use UTF-16 code units. Oxc spans use UTF-8 byte offsets; conversion rejects invalid positions and split surrogate pairs. Requests and notifications are served in order. Background cleanup drains the ordered request queue before validating its snapshot, so preview responses cannot overtake pending buffer updates. Code actions carry the snapshot version through `WorkspaceEdit.documentChanges`.

Configuration is deserialized into a strict settings struct, then validated. Custom logger names accept only a conservative ASCII identifier path. Strings are escaped before insertion. Server stdout is reserved for LSP framing.

## Syntax and edits

Each code-action request parses its snapshot with Oxc and builds semantic nodes for parent traversal and syntax checks. Expression selection accepts identifiers, `this`, conservative property reads, and selected TypeScript wrappers. Insertion uses complete statements and supported scope bodies. Unsupported transformations are withheld.

Existing text is edited by byte spans rather than reprinted. Each resulting action is reparsed to reject syntax-breaking edits. Reparsing alone cannot prove semantic equivalence. Loop-header bindings must resolve to the same symbol in the body; optional-chain descendants and unresolved `typeof` operands are withheld. Bulk actions are withheld if reparsing changes the boundaries of any untouched statement, including valid ASI merges caused by commenting, deleting, or enabling logs.

The ownership scanner requires a real marker comment immediately followed by a supported call expression statement. Disabled line comments are reconstructed and parsed separately. The scanner ignores marker-like strings and arbitrary prose. Label correction replaces only string literals; table logs have no labels to correct.

## Workspace cleanup

The `source.zedConsoleLog.deleteWorkspace` code action invokes `zedConsoleLog.deleteWorkspace` through `workspace/executeCommand`. A background scan uses the `ignore` crate to honor ignore rules and filters out symlinks, dependency/build directories, and unsupported file types. It covers existing files in registered workspace folders, deduplicating nested roots. Per-file deletion uses the same core ownership, syntax, and ASI checks as document cleanup.

A confirmation request lists affected files and log counts before `workspace/applyEdit` is sent. The ordered protocol loop remains responsive during the prompt. Open documents use synchronized text and versions; invalid buffers never fall back to disk. Buffer/folder revisions and closed-file contents are rechecked after confirmation. Edits are applied by the client, without direct filesystem writes or automatic saves. Client rejection is reported; atomicity and undo behavior depend on the editor. The command rejects concurrent cleanups, caps scans at 20,000 eligible files and 64 MiB of source, and skips files larger than 4 MiB.

## Deliberate limits

Parsing is currently per request, without a cross-request AST cache. There is no project-wide index, automatic cleanup on save, general Zed command API integration, or Python/PHP parser adapter. Rust memory safety does not establish the semantic correctness of generated edits.

## Primary references

- [Zed language-server extensions](https://zed.dev/docs/extensions/languages)
- [Zed extension development](https://zed.dev/docs/extensions/developing-extensions)
- [Oxc parser](https://oxc.rs/docs/guide/usage/parser)
- [tower-lsp-server](https://docs.rs/tower-lsp-server/0.23.0/tower_lsp_server/)
- [LSP specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
