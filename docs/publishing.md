# Publishing Console Log LSP

The public source repository is <https://github.com/MARVElOUS-DEV/zed-console-log>.
The registry ID is `debug-log-lsp`; the server name, settings key, executable, and ownership marker remain `zed-console-log`. The registry builds the WASM launcher from `extension/`; the native server is downloaded separately using Zed's API. `extension/LICENSE` must match the root MIT license.

## Build and publish native assets

Use GitHub CLI authenticated as an account with write access to the source repository. Keep the workspace Cargo version, extension manifest version, and release tag aligned. The launcher uses `v` followed by its Cargo version rather than the latest release.

| Host | Rust target | Archive |
| --- | --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` | `.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz` |
| Linux ARM64 | `aarch64-unknown-linux-musl` | `.tar.gz` |
| Linux x86-64 | `x86_64-unknown-linux-musl` | `.tar.gz` |
| Windows ARM64 | `aarch64-pc-windows-msvc` | `.zip` |
| Windows x86-64 | `x86_64-pc-windows-msvc` | `.zip` |

Archives are named `zed-console-log-v<version>-<target>.<archive>` and contain the executable at the archive root alongside `LICENSE`, `README.md`, and `CHANGELOG.md`. Each has a `.sha256` sidecar; `SHA256SUMS` covers all six archives. There is no native executable bundled into the registry extension.

Before tagging:

```sh
python3 tools/release.py validate
python3 -m unittest discover -s tools -p 'test_*.py'
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
cargo build -p zed-console-log-extension --target wasm32-wasip2 --release --locked
```

For an optional packaging rehearsal, run **Build and publish releases** manually on the release branch. This tests and packages all six targets as CI artifacts without creating a public release. Every platform is tested on a native runner. Linux builds require `musl-tools`.

Commit the reviewed release tree, push it to `main`, then create and push the version tag:

```sh
git push origin main
git tag v0.1.0
git push origin v0.1.0
gh run list --workflow release.yml
```

The workflow checks versions and the license, runs formatting and Clippy, builds WASM, runs workspace tests on all six native targets, and packages their release binaries. Only after every build and archive check succeeds does it create a draft release, upload the complete set, and publish it. The tagged commit must be reachable from the default branch. Only the publishing job has repository write permission.

If publishing fails after creating a draft, inspect it with `gh release view v0.1.0`. Keep incomplete releases as drafts. Recover by inspecting the workflow failure and uploading the missing verified files to the draft before publishing it, or delete that draft through GitHub and rerun the failed workflow. The workflow intentionally does not overwrite existing release assets.

After publishing, download the archive for your host, check its SHA-256 against `SHA256SUMS`, and run the extracted executable with `--version`. Inspect the release to confirm that all six archives, their checksum sidecars, and `SHA256SUMS` are public.

## Required Zed smoke test

Perform this test on the exact commit that the registry submodule will reference, after its native release is available. Record the commit SHA (`git rev-parse HEAD`), Zed version, OS, architecture, and date in the registry PR. Automated protocol tests and successful WASM compilation do not replace this manual test.

1. Install `extension/` using Zed's **Install Dev Extension**. If an older dev install uses `zed-console-log-lsp`, uninstall it first so two launchers cannot register the same server.
2. Merge `examples/zed-settings.jsonc`. Remove any `lsp.zed-console-log.binary.path` override for the download test, and use an environment without a `zed-console-log` PATH executable. Keep `"..."` in the language-server lists.
3. Open JavaScript, JSX, TypeScript, and TSX buffers. Confirm the language server starts from the public release. Restart it and confirm the cached installation starts; repeat offline to check cache reuse.
4. Insert logs for a variable and a supported property selection, including an unsaved buffer and a UTF-16 character before the selection. Check all six methods and a configured custom logger. Change the prefix, quotes, and method list and check that settings reach the server.
5. Comment, uncomment, correct labels, and delete generated logs for a selection and an entire document. Confirm ordinary application logs survive and undo restores the buffer.
6. Run workspace cleanup. Review filenames and counts, cancel once, then confirm with both unsaved and closed files present. Check ignored directories and ordinary logs survive. Save only when desired.
7. Confirm the existing JavaScript/TypeScript server still provides completion, diagnostics, and go-to-definition alongside Console Log actions.
8. Set `binary.path` to a local build and verify the override works with custom arguments/environment. Restore the download configuration afterward.

Manual verification is currently **pending**. Do not claim it passed or submit the registry PR until these checks have actually been performed on the submitted SHA. If you change any submitted file afterward, repeat the smoke test on the new commit.

## Registry submission

Check the live registry and open PRs for an existing `debug-log-lsp` entry before submitting. The ID was absent from both when this integration was prepared; availability is not reserved. The previously considered `console-log-lsp` ID is requested by [another author's open PR](https://github.com/zed-industries/extensions/pull/7424), which also provides logging actions. Review its status and Zed's unique-functionality requirement before submitting; a different ID alone does not settle functional overlap.

Fork `zed-industries/extensions` to your personal account, clone your fork, and create a branch:

```sh
gh repo fork zed-industries/extensions --clone
cd extensions
git switch -c add-debug-log-lsp
git submodule add https://github.com/MARVElOUS-DEV/zed-console-log.git extensions/debug-log-lsp
git -C extensions/debug-log-lsp checkout <manually-tested-commit-sha>
```

Use the exact manually tested commit, which must be reachable on the source repository's default branch. Add this top-level `extensions.toml` entry:

```toml
[debug-log-lsp]
submodule = "extensions/debug-log-lsp"
path = "extension"
version = "0.1.0"
```

Install the registry's dependencies if needed, then sort and commit:

```sh
pnpm install --frozen-lockfile
pnpm sort-extensions
git add .gitmodules extensions.toml extensions/debug-log-lsp
git commit -m "Add debug-log-lsp"
git push -u origin add-debug-log-lsp
```

The upstream [AI policy](https://github.com/zed-industries/extensions/blob/main/AI_POLICY.md) says, "Autonomous agents are not allowed to be used for contributing to this project." It also requires the PR body and maintainer replies in the contributor's own words. A human must review the registry changes and open the PR; do not have an agent submit it or copy an AI-written PR description.

In your own words, describe the JS/TS logging actions and settings, link the public native release, and report the exact tested commit, editor version, platforms, and results. Complete the upstream PR template. Submit one extension per PR and respond to review feedback within three weeks.

Requirements: [prerequisites](https://zed.dev/docs/extensions/publishing/prerequisites), [license](https://zed.dev/docs/extensions/publishing/license-requirements), and [submission guide](https://zed.dev/docs/extensions/publishing/publishing-guide).
