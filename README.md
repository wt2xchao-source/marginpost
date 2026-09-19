<p align="center">
  <img src="assets/brand/marginpost-mark.png" alt="MarginPost mark" width="96">
</p>

<h1 align="center">MarginPost</h1>

<p align="center">
  在编码 Agent 对 Markdown 的修改写入磁盘前完成审阅。<br>
  Review Markdown changes made by coding agents before they reach disk.
</p>

> V0.1 本地候选版本，尚未公开发布，也未面向普通用户提供安装包。<br>
> V0.1 local release candidate. It has not been published or packaged for
> general distribution.

MarginPost 是一个本地优先的 Markdown 工作区，用于审阅编码 Agent、脚本和其他外部工具对文件所做的修改。

MarginPost is a local-first Markdown workspace for reviewing changes made by
coding agents, scripts, and other external tools.

它面向每天使用 Markdown、但不希望通过 Git Diff 才能理解文件变化的用户。

It is designed for people who work with Markdown every day but do not want to
read Git diffs to understand what changed.

## 产品方向 / Product Direction

MarginPost 将以下能力整合在同一个工作区中：

The product combines:

- 安静、专注的 Markdown 编辑器 / A calm Markdown editor
- 持续收集外部修改的收件箱 / A persistent inbox for external changes
- 面向段落和句子的审阅方式 / Paragraph- and sentence-oriented review
- 接受、拒绝和恢复控制 / Accept, reject, and recovery controls
- 本地版本历史 / Local version history

产品的核心差异不只是展示 Diff，而是把外部修改捕获为可审阅的工作项，并放回文档上下文中呈现。

The core distinction is not simply displaying a diff. Changes are captured as
reviewable work items and presented in document context.

## 产品演示 / Product Tour

### 在上下文中审阅外部修改 / Review external changes in context

![MarginPost change review](assets/screenshots/change-review.jpg)

### 在同一工作区继续编辑 / Keep editing in the same workspace

![MarginPost Markdown editor](assets/screenshots/editor.jpg)

### 恢复早期版本 / Recover earlier versions

![MarginPost version history](assets/screenshots/history.jpg)

## V0.1 范围 / V0.1 Scope

- 打开本地文件夹 / Open a local folder
- 将本地文件夹或 Markdown 文件拖入桌面窗口 / Drop a local folder or Markdown file into the desktop window
- 基础 Markdown 编辑 / Basic Markdown editing
- 检测外部文件变化 / Detect external file changes
- 创建 Change Set / Create change sets
- 句子级和段落级 Diff / Sentence- and paragraph-level diff
- 接受或拒绝单处修改 / Accept or reject individual changes
- 接受或拒绝整个 Change Set / Accept or reject an entire change set
- 版本历史与恢复 / Version history and recovery

## 明确不做 / Explicitly Out of Scope

V0.1 不包含 AI 写作、账号、云同步、多人协作、主题商城、复杂导出、Agent 身份推断、多 Agent 冲突解决、自动生成修改理由或风险评分。只有外部工具通过已公开的本地 Hook 主动报告时，才会显示可选的来源标签。

V0.1 does not include AI writing, accounts, cloud sync, team collaboration,
themes, complex export, inferred agent identity detection, multi-agent
conflict resolution, generated change reasons, or risk scoring. Optional
source labels only appear when an external tool explicitly self-reports
through the documented local hook.

## 当前状态 / Current Status

V0.1 完整链路已通过本地验收，包括工作区编辑、外部修改捕获、待处理 Change Set 持久化、结构化审阅、单处与整体裁决、磁盘冲突保护、SQLite 版本历史、版本预览和安全恢复。恢复历史版本前，MarginPost 会先保存当前磁盘内容，并记录本次恢复操作。界面支持中英文切换，并会记住本地语言偏好。在桌面端，拖入文件夹会将其作为工作区打开；拖入单个 Markdown 文件会打开其所在文件夹并选中该文件。

The complete V0.1 path has passed local acceptance: workspace editing, external
change capture, persistent pending Change Sets, structured review, individual
and complete decisions, disk conflict protection, SQLite version history,
version preview, and safe restoration. Restoring a version first preserves the
current disk content and records the restore itself. The interface can switch
between Chinese and English and remembers the local preference. On desktop, a
folder can be dropped to open it as the Workspace, while dropping one Markdown
file opens its parent Workspace and selects that file.

当前验收范围仅限 macOS 本地环境。Windows 和 Linux 打包、签名、安装程序行为及公开分发尚未验证。拖拽入口及其自动化测试已经完成，但仍需进行一次真实的 Finder 到应用窗口拖拽确认。

Current acceptance is macOS-local. Windows and Linux packaging, signing,
installer behavior, and public distribution remain unverified. The drag-entry
implementation and automated coverage are complete; one physical
Finder-to-window drag check remains for manual confirmation.

## 已知限制 / Known Limitations

- 目前只验证了 macOS 本地开发环境和候选版本构建。<br>
  Only local macOS development and release-candidate builds are verified.
- Windows、Linux 构建，签名或公证安装包，以及公开更新分发尚未验证。<br>
  Windows and Linux builds, signed/notarized installers, and public update delivery are not yet verified.
- 尚未测试大文件和大型工作区的性能。<br>
  Large-file and large-workspace performance has not been benchmarked.
- 尚未完成正式的屏幕阅读器和纯键盘无障碍审计。<br>
  A formal screen-reader and keyboard-only accessibility audit is pending.
- Agent 来源标签依赖本地 Hook 主动报告；MarginPost 不会推断是哪个进程修改了文件。<br>
  Agent source labels depend on explicit local hook reporting; MarginPost does not infer which process changed a file.

## 技术方向 / Technology Direction

- Tauri
- React 和 TypeScript / React and TypeScript
- CodeMirror 6
- Rust 文件系统服务 / Rust filesystem services
- SQLite
- 可替换的 Diff 引擎 / Replaceable diff engine

## 快速开始 / Quick Start

MarginPost 是一款桌面应用，**不需要账号、API Key 或网络连接**，所有数据都在本地处理。

MarginPost is a desktop app with **no accounts, no API keys, and no
network requirements** — everything runs locally.

环境要求 / Prerequisites:

- macOS（Windows 和 Linux 尚未验证，请参阅“当前状态”）<br>
  macOS (Windows and Linux are unverified; see Current Status)
- [Node.js](https://nodejs.org/) ≥ 22.12
- [Rust](https://rustup.rs/) 1.98.1（由 `rust-toolchain.toml` 自动选择 / automatically selected by `rust-toolchain.toml`）
- Xcode Command Line Tools (`xcode-select --install`)

构建并运行开发版本 / Build and run the development version:

```bash
git clone https://github.com/wt2xchao-source/marginpost.git
cd marginpost
npm ci
npm run tauri dev
```

运行测试 / Run the test suites:

```bash
npm run typecheck
npm test
npm run build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo check --locked --manifest-path src-tauri/Cargo.toml
```

在全新检出的仓库中，必须先完成前端构建，再运行 Rust 检查，因为 Tauri 配置引用了 `dist/`。

The frontend build must run before the Rust checks in a clean checkout because
the Tauri configuration references `dist/`.

上述仓库地址将在项目公开发布后生效；在此之前，请使用本地副本。

The repository URL above takes effect at public publication; until then clone
from your local copy.

## 开源协议 / License

版权所有 © 2026 MarginPost contributors。

Copyright 2026 MarginPost contributors.

本项目采用 [Apache License, Version 2.0](LICENSE)。除非符合该协议，否则不得使用本项目；完整条款请参阅 `LICENSE` 文件。选择 Apache-2.0 是因为它包含明确的专利授权，适合未来可能在开放核心之上扩展商业能力的项目。

Licensed under the [Apache License, Version 2.0](LICENSE). You may not use this
project except in compliance with the License; see the `LICENSE` file for the
full text. Apache-2.0 was chosen for its express patent grant, which fits a
project that may grow commercial extensions on top of an open core.

可分发应用所使用的第三方许可证文本汇总在 `THIRD_PARTY_LICENSES.txt` 中。依赖发生变化后，请使用以下命令重新生成：

Third-party license texts used by the distributable application are collected
in `THIRD_PARTY_LICENSES.txt`. Regenerate that file after dependency changes
with:

```bash
cargo install cargo-about --version 0.9.1 --locked --features cli
npm run licenses:generate
```

## 商标 / Trademarks

MarginPost™ 和 MarginPost 标志是项目所有者尚未注册的商标。Apache-2.0 协议适用于源代码，但除合理、惯常地描述本项目外，并不授予使用 MarginPost 名称或标志的许可。

MarginPost™ and the MarginPost logo are unregistered trademarks of the project
owner. The Apache-2.0 license applies to the source code and does not grant
permission to use the MarginPost name or logo except as required for reasonable
and customary use in describing the project.

## 公开发布 / Public Release

项目尚未公开发布，当前开源协议为 Apache-2.0。公开前仍需确认仓库治理、安全政策、商标与包名核查以及发布产物。

No public release has been made. The license is Apache-2.0 (see above).
Repository governance, security policy, trademark and package-name clearance,
and release artifacts must be confirmed before publishing.
