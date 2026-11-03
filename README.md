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

1. Once the registry submission is accepted, install **Console Log LSP** (`debug-log-lsp`) from Zed's Extensions view. During development, choose **Install Dev Extension** and select this repository's `extension/` directory instead.
2. Merge [examples/zed-settings.jsonc](examples/zed-settings.jsonc) into your Zed settings. Keep `"..."` in each language-server list to retain your existing servers.
3. Open a JS/TS/JSX/TSX buffer. The launcher downloads the matching native server from [GitHub Releases](https://github.com/MARVElOUS-DEV/zed-console-log/releases) when no local override or PATH binary is available. The release must be published before this first download can succeed.
4. Put the caret on a variable or select a supported expression, and open the code actions menu.

Automatic installation supports macOS, Linux, and Windows on x86-64 and ARM64. Linux archives use musl. The downloaded server is pinned to the extension version (`0.1.0` uses release `v0.1.0`) and cached in Zed's extension work directory, so later starts work offline. Updating the extension selects a new versioned cache. Downloads use a staging directory; failed or interrupted installations are retried on the next server start. Release archives contain the executable, README, changelog, and MIT license; checksums are published alongside them.

For local development or a manually installed server, build the native executable and set an absolute override in your settings:

```json
{
  "lsp": {
    "zed-console-log": {
      "binary": {
        "path": "/absolute/path/to/zed-console-log/target/release/zed-console-log",
        "arguments": ["--stdio"]
      }
    }
  }
}
```

Binary resolution is `binary.path`, then `zed-console-log` on the worktree's PATH, then the versioned download. Explicit arguments (including an empty list) and environment overrides are preserved. No Rust or Node.js installation is needed when using a release binary. Build prerequisites apply when developing the extension.

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

To clean all registered workspace folders, leave the selection empty and choose **Delete generated logs in workspace…** from code actions (`Cmd+.` on macOS, `Ctrl+.` on Windows/Linux with the default keymap). Review the affected file paths and per-file log counts, then choose **Delete logs** or **Cancel**. The server sends a multi-file edit to the editor; it never writes files or saves buffers directly. Save the edited buffers afterward.

Workspace cleanup scans existing `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, and `.cts` files. It respects `.gitignore`, `.ignore`, and Git exclude rules; skips symlinks and dependency/build directories (`node_modules`, `target`, `dist`, `build`, `coverage`, `.next`, `.nuxt`, `vendor`, `.git`); and uses unsaved open-buffer contents instead of disk. Files with invalid synchronization, invalid syntax, or unsafe deletion boundaries are skipped and reported. Files not yet created on disk, including untitled buffers, are outside the scan. Scans skip files over 4 MiB and stop entirely above 20,000 eligible files or 64 MiB of source. Any open-buffer or workspace-folder change invalidates the preview; changed closed-file contents are also checked before applying. The action requires a local workspace root and client support for applying versioned workspace edits.

The language-server settings key and ownership marker remain `zed-console-log`, independently of the registry ID `debug-log-lsp`. Native and WebAssembly builds are validated separately; an interactive Zed smoke test of the submission commit remains part of the [publishing checklist](docs/publishing.md).

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

See [CONTRIBUTING.md](CONTRIBUTING.md), [architecture](docs/architecture.md), [maintenance](docs/maintenance.md), and the [changelog](CHANGELOG.md).
