# Changelog

## 0.1.0 — Unreleased

- Replace the initial TypeScript scaffold with a Rust Cargo workspace.
- Implement native stdio LSP synchronization, validated settings, and versioned code actions.
- Add conservative JS/TS/JSX/TSX insertion and owned-log bulk operations.
- Add workspace-wide owned-log cleanup with ignore-aware scanning, file/count previews, confirmation, and editor-applied edits.
- Add a Rust/WebAssembly Zed launcher with local binary overrides and settings forwarding.
- Add version-pinned native server downloads with offline caching and interrupted-install recovery.
- Prepare the `debug-log-lsp` registry manifest, extension-local MIT license, and six-platform GitHub Release workflow with verified archives and checksums.
- Build the WASM launcher with the registry's Rust 1.90 toolchain, independently of the native server's Rust 1.92 minimum.
- Add black-box protocol checks, CI, artifact packaging, and development documentation.

This development version has not been published or interactively verified in Zed.
