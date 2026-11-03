# Maintenance and compatibility

Version 0.1.0 is a development version. Long-term maintenance is a goal; no LTS duration, response SLA, or production support guarantee is established.

| Component | Current baseline |
| --- | --- |
| Rust | Workspace MSRV 1.92; locally validated with 1.95 |
| Native host | macOS Apple Silicon validated locally |
| Additional hosts | Linux and Windows checks configured in CI; execution pending |
| Zed extension API | Published crate 0.7.0 |
| Extension target | wasm32-wasip2 |
| LSP | Incremental sync, UTF-16 positions, code actions, versioned edits |
| Languages | JavaScript, TypeScript, JSX, TSX |
| Oxc | Pinned 0.115.0 |
| Editor integration | Native/WASM builds checked; interactive Zed smoke test pending |

## Feature matrix

| Workflow | Status |
| --- | --- |
| Insert six console methods and custom logger | Implemented for supported scopes |
| Filename/line/function/class label context | Implemented when context is available |
| Comment/uncomment/delete generated logs | Implemented with ownership and boundary checks |
| Correct generated labels | Implemented; preserves source formatting |
| Unsaved buffers and incremental edits | Implemented |
| Unbraced body and concise arrow transformations | Withheld |
| Python/PHP, workspace log tree, direct operation shortcuts, multicursor parity | Not implemented |

## Release process

1. Review behavior changes and update the changelog, docs, workspace version, extension manifest version, and lockfile as needed.
2. Run formatting, protocol tests, Clippy, native release builds, and the WebAssembly release build.
3. Install the development extension in a supported Zed version; verify insertion, settings forwarding, coexistence with the main JS/TS server, and all bulk operations.
4. Run the manual artifact workflow to build native archives and checksums, then inspect their contents. The workflow uploads CI artifacts and does not create a public release.
5. Establish a remote repository, verified platform/Zed compatibility matrix, security contact, and release destination before publishing. Do not invent registry or download URLs.

Cargo packages are marked non-publishable during development. Dependencies are locked; Dependabot proposes Cargo and workflow updates. Treat parser updates as behavior changes and rerun scope, Unicode, ownership, and edit-preservation scenarios before merging.
