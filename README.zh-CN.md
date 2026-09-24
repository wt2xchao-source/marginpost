<p align="center">
  <strong>简体中文</strong> · <a href="README.md">English</a>
</p>

<p align="center">
  <img src="assets/brand/marginpost-mark.png" alt="MarginPost 标志" width="96">
</p>

<h1 align="center">MarginPost</h1>

<p align="center">
  外部工具写入 Markdown 后，在其成为工作区基线前完成审阅。
</p>

> V0.3 公开预览版。源代码及使用 ad-hoc 签名的 Apple Silicon macOS 安装包现已开放测试。

MarginPost 是一个本地优先的 Markdown 工作区，用于审阅编码 Agent、脚本和其他外部工具对文件所做的修改。

它面向每天使用 Markdown、但不希望通过 Git Diff 才能理解文件变化的用户。

## 公开测试：欢迎反馈

MarginPost V0.3 目前是测试版本，并非正式生产版本。

测试时请优先使用非关键 Markdown 文件的副本，并为重要内容保留备份。我们尤其希望了解：

- 外部修改是否被准确捕获；
- 审阅、接受和拒绝修改是否清晰顺手；
- 版本历史与恢复功能是否让人放心；
- 是否出现崩溃、性能问题或难以理解的行为；
- MarginPost 还缺少哪些真实工作流。

[报告 Bug](https://github.com/wt2xchao-source/marginpost/issues/new?template=bug_report.yml)
·
[提出改进建议](https://github.com/wt2xchao-source/marginpost/issues/new?template=improvement.yml)
·
[参与讨论](https://github.com/wt2xchao-source/marginpost/discussions)

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

### 两分钟演示

[观看带英文旁白和内嵌中文字幕的 V0.3 完整演示](assets/demo/marginpost-demo-v0.3.0.mp4)

[英文字幕](assets/demo/marginpost-demo-v0.3.0-en.srt)
·
[中文字幕](assets/demo/marginpost-demo-v0.3.0-zh-CN.srt)

## V0.3 范围

- 打开本地文件夹
- 搜索工作区中的 Markdown 文件
- 将本地文件夹或 Markdown 文件拖入桌面窗口
- 基础 Markdown 编辑
- 检测外部文件变化
- 检测 Markdown 新建、删除和重命名事件
- 创建 Change Set
- 基于 GFM/CommonMark AST 的结构化块比较
- 接受或拒绝单处修改
- 接受或拒绝整个 Change Set
- 使用按钮或快捷键切换修改，并撤销上一次裁决
- 同一文件再次变化时，将旧候选明确标记为已被替代
- 磁盘冲突后显示过期 Change Set
- 筛选待审阅或全部 Change Set
- 版本历史、结构化版本比较与恢复

MarginPost 不是写入前沙箱。外部工具会先写入磁盘，MarginPost 捕获真实的文件系统结果，并在执行审阅裁决前再次核验候选状态。详见 [Change Set 生命周期](docs/CHANGE_SET_LIFECYCLE.md)。

## 明确不做

V0.3 不包含 AI 写作、账号、云同步、多人协作、主题商城、复杂导出、Agent 身份推断、多 Agent 冲突解决、自动生成修改理由或风险评分。只有外部工具通过已公开的本地 Hook 主动报告时，才会显示可选的来源标签。详见 [Agent Hook 使用文档](docs/AGENT_HOOKS.md)。

## 当前状态

V0.3 完整链路已通过本地自动化验收，包括工作区编辑、外部修改捕获、待处理 Change Set 持久化、结构化审阅、单处与整体裁决、新建/删除/重命名语义、过期与已被替代状态、SQLite 版本历史、相邻版本结构化比较和安全恢复。恢复历史版本前，MarginPost 会先保存当前磁盘内容，并记录本次恢复操作。

V0.3.0 GitHub Release 提供使用 ad-hoc 签名的 Apple Silicon macOS `.dmg`。它没有使用 Apple Developer ID 签名，也没有经过公证。CI 已配置 Windows 和 Linux 源码验证任务；在 GitHub 托管任务通过前，相关支持仍属于暂定状态。

## 已知限制

- macOS 安装包仅支持 Apple Silicon，使用 ad-hoc 签名且未公证。
- 暂不提供 Windows 和 Linux 桌面安装包。
- 尚未实现自动更新。
- 尚未测试大文件和大型工作区的性能。
- 已实现审阅快捷键，但尚未完成正式的屏幕阅读器及完整纯键盘无障碍审计。
- Agent 来源标签依赖本地 Hook 主动报告；MarginPost 不会推断是哪个进程修改了文件。

## 技术方向

- Tauri
- React 和 TypeScript
- CodeMirror 6
- Rust 文件系统服务
- SQLite
- `markdown` mdast 解析器和可替换的 Diff 引擎

## 在 macOS 上安装

[从 GitHub Releases 下载 MarginPost V0.3.0](https://github.com/wt2xchao-source/marginpost/releases/tag/v0.3.0)。

安装包仅适用于 Apple Silicon Mac，使用 ad-hoc 签名，没有使用 Apple Developer ID 签名，也没有经过公证。macOS 首次启动时可能拦截应用。请在 Finder 中按住 Control 点击应用，选择“打开”并确认警告。不要全局关闭 Gatekeeper。

## 从源码运行

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

MarginPost V0.3.0 当前以 Apache-2.0 公开预览版开放。Bug 和产品反馈请提交至 GitHub Issues；安全漏洞请按照 `SECURITY.md` 报告。
