<p align="center">
  <a href="README.zh-CN.md">简体中文</a> · <strong>English</strong>
</p>

<p align="center">
  <img src="assets/brand/marginpost-mark.png" alt="MarginPost mark" width="96">
</p>

<h1 align="center">MarginPost</h1>

<p align="center">
  Review Markdown changes after external tools write them, before you accept
  them as the workspace baseline.
</p>

> V0.3 public preview. Source code and an ad-hoc signed Apple Silicon macOS
> package are available for testing.

MarginPost is a local-first Markdown workspace for reviewing
changes made by coding agents, scripts, and other external tools.

It is designed for people who work with Markdown every day but do not want to
read Git diffs to understand what changed.

## Public Preview: Your Feedback Matters

MarginPost V0.3 is a testing release, not a production-ready application.
Please test with copies of non-critical Markdown files and keep backups of
important work.

[Report a bug](https://github.com/wt2xchao-source/marginpost/issues/new?template=bug_report.yml)
·
[Suggest an improvement](https://github.com/wt2xchao-source/marginpost/issues/new?template=improvement.yml)
·
[Join the discussion](https://github.com/wt2xchao-source/marginpost/discussions)

## Product Direction

The product combines:

- a calm Markdown editor;
- a persistent inbox for external changes;
- paragraph- and sentence-oriented review;
- accept, reject, and recovery controls;
- local version history.

The core distinction is not simply displaying a diff. Changes are captured as
reviewable work items and presented in document context.

## Product Tour

### Review external changes in context

![MarginPost change review](assets/screenshots/change-review.jpg)

### Keep editing in the same workspace

![MarginPost Markdown editor](assets/screenshots/editor.jpg)

### Recover earlier versions

![MarginPost version history](assets/screenshots/history.jpg)

### Two-minute demo

[Watch the V0.3 workflow with English narration and embedded Chinese subtitles](assets/demo/marginpost-demo-v0.3.0.mp4)

[English subtitles](assets/demo/marginpost-demo-v0.3.0-en.srt)
·
[Chinese subtitles](assets/demo/marginpost-demo-v0.3.0-zh-CN.srt)

## V0.3 Scope

- Open a local folder
- Search Markdown files across the workspace
- Drop a local folder or Markdown file into the desktop window
- Basic Markdown editing
- Detect external file changes
- Detect Markdown creation, deletion, and rename events
- Create change sets
- GFM/CommonMark AST-backed block comparison
- Accept or reject individual changes
- Accept or reject an entire change set
- Navigate changes with buttons or keyboard shortcuts and undo the latest decision
- Mark older candidates as superseded when the same file changes again
- Show stale Change Sets after disk conflicts
- Filter pending or all Change Sets
- Version history, structured version comparison, and recovery

MarginPost is not a pre-write sandbox. External tools write first; MarginPost
captures the resulting local filesystem change and verifies the candidate
again before applying a review decision. See
[Change Set Lifecycle](docs/CHANGE_SET_LIFECYCLE.md).

## Explicitly Out of Scope

V0.3 does not include AI writing, accounts, cloud sync, team collaboration,
themes, complex export, inferred agent identity detection, multi-agent
conflict resolution, generated change reasons, or risk scoring. Optional
source labels only appear when an external tool explicitly self-reports
through the documented local hook. See [Agent Hooks](docs/AGENT_HOOKS.md).

## Current Status

The complete V0.3 path has passed local automated acceptance: workspace
editing, external change capture, persistent pending Change Sets, structured
review, individual and complete decisions, create/delete/rename semantics,
stale and superseded states, SQLite version history, adjacent-version
structural comparison, and safe restoration. Restoring a version first
preserves the current disk content and records the restore itself.

The V0.3.0 GitHub Release includes an ad-hoc signed Apple Silicon macOS `.dmg`.
It is not signed with an Apple Developer ID and is not notarized. Windows and
Linux source-validation jobs are configured in CI; support remains provisional
until the GitHub-hosted runs pass.

## Known Limitations

- The macOS package is Apple Silicon only, ad-hoc signed, and not notarized.
- Windows and Linux packaged applications are not provided.
- Automatic update delivery is not implemented.
- Large-file and large-workspace performance has not been benchmarked.
- Review shortcuts are implemented, but a formal screen-reader and complete
  keyboard-only accessibility audit is pending.
- Agent source labels depend on explicit local hook reporting; MarginPost does
  not infer which process changed a file.

## Technology Direction

- Tauri
- React and TypeScript
- CodeMirror 6
- Rust filesystem services
- SQLite
- `markdown` mdast parser plus replaceable diff engine

## Install On macOS

[Download MarginPost V0.3.0 from GitHub Releases](https://github.com/wt2xchao-source/marginpost/releases/tag/v0.3.0).

The package is for Apple Silicon Macs. It is ad-hoc signed, not signed with an
Apple Developer ID, and not notarized. macOS may block the first launch. In
Finder, Control-click the app, choose **Open**, and confirm the warning. Do not
disable Gatekeeper globally.

## Build From Source

MarginPost is a desktop app with **no accounts, no API keys, and no
network requirements** — everything runs locally.

Prerequisites:

- macOS (Windows and Linux are unverified; see Current Status)
- [Node.js](https://nodejs.org/) ≥ 22.12
- [Rust](https://rustup.rs/) 1.98.1 (automatically selected by
  `rust-toolchain.toml`)
- Xcode Command Line Tools (`xcode-select --install`)

Build and run the development version:

```bash
git clone https://github.com/wt2xchao-source/marginpost.git
cd marginpost
npm ci
npm run tauri dev
```

Run the test suites:

```bash
npm run typecheck
npm test
npm run build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo check --locked --manifest-path src-tauri/Cargo.toml
```

The frontend build must run before the Rust checks in a clean checkout because
the Tauri configuration references `dist/`.

## License

Copyright 2026 MarginPost contributors.

Licensed under the [Apache License, Version 2.0](LICENSE). You may not
use this project except in compliance with the License; see the
`LICENSE` file for the full text. Apache-2.0 was chosen for its express
patent grant, which fits a project that may grow commercial extensions
on top of an open core.

Third-party license texts used by the distributable application are collected
in `THIRD_PARTY_LICENSES.txt`. Regenerate that file after dependency changes
with:

```bash
cargo install cargo-about --version 0.9.1 --locked --features cli
npm run licenses:generate
```

## Trademarks

MarginPost™ and the MarginPost logo are unregistered trademarks of the project
owner. The Apache-2.0 license applies to the source code and does not grant
permission to use the MarginPost name or logo except as required for reasonable
and customary use in describing the project.

## Public Preview

MarginPost V0.3.0 is available as an Apache-2.0 public preview. Use GitHub
Issues for bugs and product feedback, and follow `SECURITY.md` for
vulnerability reports.
