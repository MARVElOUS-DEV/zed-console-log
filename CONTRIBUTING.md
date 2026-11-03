# Contributing

## Workspace

- `crates/core/src/`: validated settings, formatting, UTF-16/byte positions, Oxc syntax analysis, and byte edits.
- `crates/server/src/main.rs`: LSP lifecycle, synchronized buffers, configuration, and versioned workspace edits.
- `crates/server/tests/protocol.rs`: black-box stdio integration tests that launch the actual server.
- `extension/`: Zed extension manifest and Rust/WASM launcher.
- `examples/` and `docs/`: configuration, fixtures, architecture, and support policy.
- `target/`: ignored build output. Keep `Cargo.lock` under version control.

The explicit Rust migration supersedes the TypeScript-specific tooling and compiler guidance in the original `AGENTS.md`, which is preserved from the initial contributor-guide request.

## Development

Use Rust 1.92 or newer. Run:

```sh
cargo build --locked
cargo test --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build -p zed-console-log-extension --target wasm32-wasip2 --release --locked
```

Install `wasm32-wasip2` through rustup before the extension build. Rustfmt uses four spaces. Use snake_case for modules/functions and PascalCase for types. Keep settings names camelCase for LSP clients. Keep protocol stdout free of diagnostics.

## Behavioral checks

Prefer protocol integration scenarios over new feature unit tests. Give tests descriptive snake_case names explaining the behavior. Exercise unsaved incremental buffers, document versions, UTF-16 positions, scope, escaping, ownership, and shutdown. Add a regression scenario for meaningful edit-planning changes. No numerical coverage threshold is configured.

Never repeat side-effectful expressions, erase unowned logs, or move reads into invalid scopes. Preserve comments and surrounding source; use parsed spans instead of line-only guesses. Withhold unsupported edits.

## Changes and review

The repository has no committed history establishing a message convention. Use focused, imperative subjects such as `Preserve return scope when inserting logs`.

PRs should describe the trigger and resulting behavior, link related issues, and report the commands actually run. Include before/after source examples for edits and Zed screenshots when relevant. Record limitations and update documentation with behavior changes. Do not commit build output, credentials, or claim publication or editor verification that has not occurred.
