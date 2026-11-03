# zed-console-log handoff

Updated: 2026-11-03. Work is now in `/Users/caesar/workSpace/playyard/zed-console-log`, initialized as its own Git repository. There is no remote or committed history yet.

## Current direction

The user explicitly approved moving the server and extension to Rust. This supersedes the original TypeScript/compiler-API plan and related tooling references in the preserved `AGENTS.md`. See `CONTRIBUTING.md` for current contributor instructions.

The repository is a Cargo workspace: `crates/core` contains settings, text-position conversion, formatting, Oxc syntax analysis, and edit planning; `crates/server` implements the native stdio LSP; `extension` implements the Rust/WebAssembly Zed launcher. The initial TypeScript scaffold and Node tooling have been removed.

## Implemented behavior

- JS/TS/JSX/TSX parsing, conservative expression selection, and six console insertion actions plus a configured custom logger.
- Complete variable declarations, return/throw placement, braced function parameters, loop bindings, and catch bindings.
- Context labels, configurable quoting and terminators, escaping, CRLF/tabs/EOF preservation, and final inserted line numbers.
- Structural ownership using `/* zed-console-log */`; document/selection comment, uncomment, delete, and label-correction actions. Label correction preserves existing formatting. Table calls use data only.
- Open-buffer incremental synchronization, UTF-16 positions, stale-version rejection, invalid-snapshot handling, versioned workspace edits, action-kind filtering, strict configuration, and shutdown.
- Zed extension manifest for existing JavaScript/TypeScript/TSX languages, explicit local binary override or PATH lookup, and settings forwarding.
- Protocol integration tests, native/WASM build instructions, CI matrices, a manual artifact workflow, maintenance/feature matrix, MIT license, and security guidance.

## Safety and scope decisions

Keep this implementation independent of Turbo Console Log source and branding. It is not a licensed fork.

Never duplicate calls, assignments, awaits, increments, or dynamic subscripts just to log them. Property reads may invoke getters/proxies. Withhold unsupported transformations instead of moving reads outside scope. Conditional/default references, concise arrows, unbraced bodies, and multiline selections are currently withheld.

Bulk commenting requires a standalone owned statement. Disabled log contents are reparsed before enabling. Ordinary application logs, marker-like strings, and arbitrary marker prose must be preserved. No actions run automatically on save.

The server never reads stale disk source for an open document, and stdout is reserved for protocol frames.

## Validation and remaining work

Local validation passes 18 black-box protocol tests on macOS Apple Silicon with Rust 1.95. Formatting, strict Clippy across all workspace targets, native release compilation, and the wasm32-wasip2 release build pass. The WASM component header and Zed API-version custom section are present. A native development archive and SHA-256 checksum are in ignored target/dist/. The workspace MSRV is 1.92; CI includes an MSRV check and native Linux/macOS/Windows tests. Build and check results from the current task should be reported accurately; CI has not run remotely.

The native binary and WASM launcher must both compile before editor installation. An interactive Zed smoke test remains: install `extension/` as a dev extension, configure the absolute binary path using `examples/zed-settings.jsonc`, preserve existing language servers using `"..."`, and verify insertion, all bulk actions, and settings changes in an open buffer.

No remote URL, downloadable public binary, npm package, GitHub release, or Zed registry publication exists. The extension uses an installed/local binary and does not pretend a download is available. Add verified repository metadata before publication. The manual artifact workflow uploads build artifacts only.

Python/PHP adapters, workspace log trees, direct per-operation shortcuts, multicursor parity, unsupported syntax transformations, and proven LTS support remain future work. See `docs/maintenance.md` for the compatibility matrix and release checklist.
