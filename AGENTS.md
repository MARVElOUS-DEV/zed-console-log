# Repository Guidelines

## Project Structure & Module Organization

- `crates/core/src/`: Oxc syntax analysis, edit planning, settings validation, formatting, and text positions.
- `crates/server/src/main.rs`: native stdio LSP server and synchronized document buffers.
- `crates/server/tests/protocol.rs`: integration tests launching the real server.
- `extension/`: Zed manifest and Rust/WebAssembly launcher.
- `examples/`: source fixtures and Zed settings; `docs/`: architecture and maintenance guidance.
- `target/`: ignored build output. Keep `Cargo.lock` tracked.

## Build, Test, and Development Commands

Use Rust 1.92 or newer, installed through rustup. Run from the workspace root:

- `cargo build --release --locked`: build the native server.
- `cargo run -p zed-console-log -- --stdio`: launch locally for an LSP client.
- `cargo test --locked`: run native tests, including protocol scenarios.
- `cargo fmt --all -- --check`: check Rust formatting.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: lint all targets strictly.
- `rustup target add wasm32-wasip2`: install the extension compilation target.
- `cargo build -p zed-console-log-extension --target wasm32-wasip2 --release --locked`: build the WASM launcher.

Install `extension/` through Zed's **Install Dev Extension** and configure the server using `examples/zed-settings.jsonc`.

## Coding Style & Naming Conventions

Follow `.editorconfig`: UTF-8, LF, final newline, two-space indentation generally, four spaces for Rust. Use Rust 2024, rustfmt, `snake_case` functions/modules, and `PascalCase` types. Public JSON settings use `camelCase`. Keep protocol stdout exclusively for LSP frames.

## Testing Guidelines

Use Rust's test harness; prefer black-box stdio scenarios in `crates/server/tests/protocol.rs`. Name tests descriptively in `snake_case`. Add regression scenarios for behavioral changes, especially unsaved edits, versions, UTF-16 positions, CRLF, scope, escaping, ownership, and shutdown. No numerical coverage threshold is configured. Manually verify editor-facing changes in Zed.

## Commit & Pull Request Guidelines

The current history contains a `feat:` subject but is too small to establish a broad convention. Prefer focused, imperative subjects such as `fix: preserve statement boundaries in bulk edits`. PRs should explain the trigger and resulting behavior, link relevant issues, report checks actually run, and include before/after examples or Zed screenshots when useful. Update behavior documentation and record limitations.

## Architecture & Safety Constraints

Preserve the `/* zed-console-log */` ownership marker and ordinary application logs. Never duplicate side-effectful expressions or move reads outside valid scopes. Preserve statement boundaries; reparsing alone cannot prove semantic safety. Withhold unsupported edits. Validate configuration, restrict custom loggers to dotted identifiers, and escape generated strings. Never commit credentials or build artifacts.
