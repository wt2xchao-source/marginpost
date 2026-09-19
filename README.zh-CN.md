<p align="center">
  <strong>简体中文</strong> · <a href="README.md">English</a>
</p>

<p align="center">
  <img src="assets/brand/marginpost-mark.png" alt="MarginPost 标志" width="96">
</p>

<h1 align="center">MarginPost</h1>

<p align="center">
  在编码 Agent 对 Markdown 的修改写入磁盘前完成审阅。
</p>

> V0.1 公开预览版。源代码现已开放测试，暂不提供打包版本。

MarginPost 是一个本地优先的 Markdown 工作区，用于审阅编码 Agent、脚本和其他外部工具对文件所做的修改。

它面向每天使用 Markdown、但不希望通过 Git Diff 才能理解文件变化的用户。

## 产品方向

MarginPost 将以下能力整合在同一个工作区中：

- 安静、专注的 Markdown 编辑器
- 持续收集外部修改的收件箱
- 面向段落和句子的审阅方式
- 接受、拒绝和恢复控制
- 本地版本历史

产品的核心差异不只是展示 Diff，而是把外部修改捕获为可审阅的工作项，并放回文档上下文中呈现。

## 产品演示

### 在上下文中审阅外部修改

![MarginPost 改动审阅](assets/screenshots/change-review.jpg)

### 在同一工作区继续编辑

![MarginPost Markdown 编辑器](assets/screenshots/editor.jpg)

### 恢复早期版本

![MarginPost 版本历史](assets/screenshots/history.jpg)

## V0.1 范围

- 打开本地文件夹
- 将本地文件夹或 Markdown 文件拖入桌面窗口
- 基础 Markdown 编辑
- 检测外部文件变化
- 创建 Change Set
- 句子级和段落级 Diff
- 接受或拒绝单处修改
- 接受或拒绝整个 Change Set
- 版本历史与恢复

## 明确不做

V0.1 不包含 AI 写作、账号、云同步、多人协作、主题商城、复杂导出、Agent 身份推断、多 Agent 冲突解决、自动生成修改理由或风险评分。只有外部工具通过已公开的本地 Hook 主动报告时，才会显示可选的来源标签。

## 当前状态

V0.1 完整链路已通过本地验收，包括工作区编辑、外部修改捕获、待处理 Change Set 持久化、结构化审阅、单处与整体裁决、磁盘冲突保护、SQLite 版本历史、版本预览和安全恢复。恢复历史版本前，MarginPost 会先保存当前磁盘内容，并记录本次恢复操作。界面支持中英文切换，并会记住本地语言偏好。在桌面端，拖入文件夹会将其作为工作区打开；拖入单个 Markdown 文件会打开其所在文件夹并选中该文件。

当前验收范围仅限 macOS 本地环境。Windows 和 Linux 打包、签名、安装程序行为及公开分发尚未验证。拖拽入口及其自动化测试已经完成，但仍需进行一次真实的 Finder 到应用窗口拖拽确认。

## 已知限制

- 目前只验证了 macOS 本地开发环境和候选版本构建。
- Windows、Linux 构建，签名或公证安装包，以及公开更新分发尚未验证。
- 尚未测试大文件和大型工作区的性能。
- 尚未完成正式的屏幕阅读器和纯键盘无障碍审计。
- Agent 来源标签依赖本地 Hook 主动报告；MarginPost 不会推断是哪个进程修改了文件。

## 技术方向

- Tauri
- React 和 TypeScript
- CodeMirror 6
- Rust 文件系统服务
- SQLite
- 可替换的 Diff 引擎

## 快速开始

MarginPost 是一款桌面应用，**不需要账号、API Key 或网络连接**，所有数据都在本地处理。

环境要求：

- macOS（Windows 和 Linux 尚未验证，请参阅“当前状态”）
- [Node.js](https://nodejs.org/) ≥ 22.12
- [Rust](https://rustup.rs/) 1.98.1（由 `rust-toolchain.toml` 自动选择）
- Xcode Command Line Tools (`xcode-select --install`)

构建并运行开发版本：

```bash
git clone https://github.com/wt2xchao-source/marginpost.git
cd marginpost
npm ci
npm run tauri dev
```

运行测试：

```bash
npm run typecheck
npm test
npm run build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo check --locked --manifest-path src-tauri/Cargo.toml
```

在全新检出的仓库中，必须先完成前端构建，再运行 Rust 检查，因为 Tauri 配置引用了 `dist/`。

现在可以直接从 GitHub 克隆仓库。签名安装包和打包版本暂未提供。

## 开源协议

版权所有 © 2026 MarginPost contributors。

本项目采用 [Apache License, Version 2.0](LICENSE)。除非符合该协议，否则不得使用本项目；完整条款请参阅 `LICENSE` 文件。选择 Apache-2.0 是因为它包含明确的专利授权，适合未来可能在开放核心之上扩展商业能力的项目。

可分发应用所使用的第三方许可证文本汇总在 `THIRD_PARTY_LICENSES.txt` 中。依赖发生变化后，请使用以下命令重新生成：

```bash
cargo install cargo-about --version 0.9.1 --locked --features cli
npm run licenses:generate
```

## 商标

MarginPost™ 和 MarginPost 标志是项目所有者尚未注册的商标。Apache-2.0 协议适用于源代码，但除合理、惯常地描述本项目外，并不授予使用 MarginPost 名称或标志的许可。

## 公开预览

MarginPost 当前以 V0.1 公开预览版开放，并采用 Apache-2.0 协议。暂不提供签名安装包或打包版本。Bug 和产品反馈请提交至 GitHub Issues；安全漏洞请按照 `SECURITY.md` 报告。
