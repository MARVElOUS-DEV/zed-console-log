# Maintenance and compatibility

Version 0.1.0 is a development version. Long-term maintenance is a goal; no LTS duration, response SLA, or production support guarantee is established.

| Component | Current baseline |
| --- | --- |
| Rust | Native workspace MSRV 1.92; WASM launcher MSRV 1.90 to match registry CI |
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
| Workspace deletion | Preview and confirmation, ignore-aware scan, versioned editor edits |
| Correct generated labels | Implemented; preserves source formatting |
| Native server installation | Absolute path, PATH, or cached GitHub Release download pinned to the extension version |
| Unsaved buffers and incremental edits | Implemented |
| Unbraced body and concise arrow transformations | Withheld |
| Python/PHP, workspace log tree, direct operation shortcuts, multicursor parity | Not implemented |

## Release process

1. Review behavior changes and update the changelog, docs, workspace version, extension manifest version, and lockfile as needed.
2. Run formatting, protocol tests, Clippy, native release builds, and the WebAssembly release build.
3. Install the development extension in a supported Zed version; verify insertion, settings forwarding, coexistence with the main JS/TS server, all bulk operations, and workspace cleanup (including Cancel and unsaved buffers).
4. Push a matching `v<version>` tag to run **Build and publish releases**. It tests six native targets, builds the WASM launcher, packages native binaries and licenses, and verifies archives and checksums before publishing a GitHub Release. Running the workflow manually on a branch produces CI artifacts without publishing. See [publishing](publishing.md) for the platform matrix and exact steps.
5. Test the exact submission commit in Zed, including automatic installation from the public release, then submit the registry PR with `path = "extension"`. Record actual editor and platform results; leave unexecuted checks pending.

Cargo packages are marked non-publishable during development. Dependencies are locked; Dependabot proposes Cargo and workflow updates. Treat parser updates as behavior changes and rerun scope, Unicode, ownership, and edit-preservation scenarios before merging.
