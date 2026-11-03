# zed-console-log

A Rust language server providing syntax-aware debug logging code actions for JavaScript, TypeScript, JSX, and TSX. A small Rust/WebAssembly extension connects it to Zed alongside your existing language servers.

This is an initial development implementation, independently written and MIT licensed. It is not a Turbo Console Log fork and is not an LTS release.

## Build and check

Install Rust through [rustup](https://rustup.rs/). The workspace declares Rust 1.92 or newer; local validation currently uses Rust 1.95.

```sh
cargo build --release --locked
cargo test --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

The native executable is `target/release/zed-console-log` (`.exe` on Windows). It has no Node.js runtime dependency. Run `zed-console-log --stdio` from an LSP client, or `--version` to inspect the version.

To build the Zed extension separately:

```sh
rustup target add wasm32-wasip2
cargo build -p zed-console-log-extension --target wasm32-wasip2 --release --locked
```

## Use in Zed

1. Build the native server.
2. Open Zed's Extensions view, choose **Install Dev Extension**, and select this repository's `extension/` directory. Zed builds its WebAssembly launcher.
3. Merge [examples/zed-settings.jsonc](examples/zed-settings.jsonc) into your Zed settings, replacing the binary path with the absolute path to your executable. Keep `"..."` in each language-server list to retain your existing servers.
4. Open a JS/TS/JSX/TSX buffer, put the caret on a variable or select a supported expression, and open the code actions menu.

Choose an insertion action for `console.log`, `info`, `debug`, `warn`, `error`, or `table`. Configuring `customFunction` also offers a custom logger action. `console.table` receives data directly, without a label argument.

For example, selecting `value` in:

```ts
const value = loadValue();
```

offers an edit equivalent to:

```ts
const value = loadValue();
/* zed-console-log */ console.log('ZCL ~ example.ts:2 ~ value:', value);
```

Bulk actions comment, uncomment, delete, or correct generated labels. A nonempty selection restricts them to intersecting logs; an empty selection targets the document. Ownership uses the stable marker and parsed statement shape, independently of the display prefix. Ordinary application logs are preserved.

The launcher uses your configured `binary.path`, or finds `zed-console-log` on the worktree's PATH. It does not download an unpublished package or modify your settings. Native and WebAssembly builds are validated separately; an interactive Zed smoke test remains part of the development checklist.

## Configuration

Put these options in `lsp.zed-console-log.settings`. Other LSP clients can supply the same object as initialization options or through `workspace/didChangeConfiguration`. Nested `zed-console-log` and `zedConsoleLog` objects are also accepted.

| Option | Default | Meaning |
| --- | --- | --- |
| `prefix` | `"ZCL"` | Label prefix |
| `delimiter` | `" ~ "` | Label component separator |
| `quote` | `"single"` | `single` or `double` |
| `semicolon` | `true` | Generated terminator preference |
| `includeFileName` | `true` | Include document filename |
| `includeLineNumber` | `true` | Include the generated statement's line |
| `includeFunctionName` | `true` | Include available enclosing function name |
| `includeClassName` | `true` | Include available enclosing class name |
| `insertPosition` | `"below"` | `above` or `below` for expression statements |
| `methods` | All six methods | Console actions to offer |
| `customFunction` | `null` | ASCII dotted identifier, e.g. `logger.debug` |
| `indentSize` | `2` | Indentation for empty blocks; integer 1–16 |

Variable declarations are logged after the full declaration. Return and throw values are logged before the terminal statement. Braced parameter, loop-binding, and catch-binding scopes are supported. Explicit semicolons are retained when needed to protect statement boundaries. Label correction preserves existing statement formatting.

Unknown or invalid options are rejected. Invalid initialization options return an LSP error; invalid configuration changes retain the previous settings and report an LSP log message. Configuration updates replace the settings object, with omitted fields taking defaults.

## Current boundaries

- Initial scope is JS/TS/JSX/TSX. Python and PHP adapters are future work.
- Calls, assignments, awaits, increments, dynamic subscripts, and optional chains are not accepted as whole selected expressions.
- Property reads can invoke getters or proxies. Logging adds an evaluation; use it deliberately.
- Concise arrows, unbraced control flow, conditional/default references, multiline selected expressions, and unsupported syntax contexts withhold insertion actions.
- Buffers with parser or semantic syntax errors withhold actions until repaired.
- Bulk commenting requires a standalone log without trailing application code. Inline owned logs may still be deleted or have their labels corrected.
- The server uses open-buffer contents and versioned edits, with UTF-16 LSP positions. A malformed incremental update invalidates its snapshot until the buffer is reopened.
- Actions do not run automatically on save. Workspace log trees, direct operation shortcuts, and multicursor parity are not implemented.

See [CONTRIBUTING.md](CONTRIBUTING.md), [architecture](docs/architecture.md), [maintenance](docs/maintenance.md), and the current [HANDOFF.md](HANDOFF.md).
