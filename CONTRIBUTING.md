# Contributing to MarginPost

MarginPost is a local-first desktop application. Contributions should preserve
local data ownership, explicit review decisions, and predictable file safety.

## Development Setup

Requirements:

- Node.js 22.12 or newer
- Rust 1.98.1 (selected by `rust-toolchain.toml`)
- Platform prerequisites required by Tauri

Install and run:

```bash
npm ci
npm run tauri dev
```

## Required Checks

Run these before opening a pull request:

```bash
npm run typecheck
npm test
npm run build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo check --locked --manifest-path src-tauri/Cargo.toml
```

Keep this order in a clean checkout: Tauri's compile-time configuration
expects the frontend `dist/` directory created by `npm run build`.

## Contribution Rules

- Keep changes focused on one problem.
- Add or update tests for behavior changes.
- Do not add telemetry, network requests, accounts, or cloud storage without a
  reviewed product and security decision.
- Do not commit API keys, tokens, local databases, logs, environment files, or
  machine-specific paths.
- Preserve user files and fail safely when disk content changes unexpectedly.

For security issues, follow `SECURITY.md` instead of opening a public issue.
