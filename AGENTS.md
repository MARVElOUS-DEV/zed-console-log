# Repository Guidelines

## Project Structure & Module Organization

This repository is an early scaffold for a syntax-aware logging language server for Zed and other LSP editors.

- `src/config.ts`: settings defaults and validation.
- `src/syntax.ts`: TypeScript AST parsing, expression selection, and edit helpers.
- `src/format.ts`: log labels, escaping, and statement formatting.
- `integration/`: reserved for protocol tests.
- `extension/src/`: reserved for the Rust/WASM Zed launcher.
- `docs/`, `examples/`, and `.github/workflows/`: currently empty.
- `dist/`: generated TypeScript output; excluded from version control.

Read `HANDOFF.md` for design decisions and outstanding implementation work. Some workspace details there are historical; verify against the current filesystem.

## Build, Test, and Development Commands

Use Node.js 22 or newer and Yarn 1.22.22.

- `yarn install`: install dependencies.
- `yarn build`: compile TypeScript into `dist/`.
- `yarn dev`: compile continuously in watch mode.
- `yarn typecheck`: check types without emitting files.
- `yarn eslint`: lint source and integration scripts.
- `yarn test:integration`: build, then run `integration/*.mjs` with Node’s test runner.
- `yarn check`: run type checking, integration tests, and linting.
- `yarn start`: launch the planned stdio server.

The server entry point and integration tests are not implemented yet; their commands cannot currently validate a working server.

## Coding Style & Naming Conventions

Follow `.editorconfig`: UTF-8, LF, final newline, two-space indentation; Rust uses four spaces. TypeScript uses strict checking, NodeNext modules, single quotes, and semicolons. Use camelCase for functions and variables, PascalCase for types, and explicit `.js` extensions for local ESM imports. Run ESLint; no formatter is configured.

## Testing Guidelines

Add black-box stdio LSP scenarios as `integration/*.mjs` using Node’s built-in test runner. Prioritize unsaved edits, insertion scope, side-effect refusal, configuration errors, CRLF, Unicode positions, and clean shutdown. Follow the handoff’s preference for protocol integration checks over new feature unit tests. No coverage threshold is configured.

## Commit & Pull Request Guidelines

No Git metadata or commit history is available in this workspace. Use concise, imperative commit subjects describing one focused change. PRs should explain behavior, link relevant issues, report validation results and limitations, and include Zed screenshots when editor behavior changes.

## Architecture & Safety Constraints

Use the TypeScript compiler API for syntax analysis. Preserve the stable `/* zed-console-log */` ownership marker and ordinary application logs. Never duplicate side-effectful expressions. Validate settings, restrict custom loggers to dotted identifiers, and escape generated strings. Reserve server stdout for LSP messages.
