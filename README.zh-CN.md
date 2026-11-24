<p align="center">
  <img src="./crates/winc-app/assets/icon.svg" alt="obnote" width="128" />
</p>

<h1 align="center">obnote</h1>

<p align="center">
  极速、极省内存的纯 Rust 本地知识库与办公资料管理器（Obsidian-lite）
</p>

<p align="center">
  <a href="./README.md">English</a> | 简体中文
</p>

<p align="center">
  <a href="./LICENSE"><img alt="License" src="https://img.shields.io/badge/License-Apache%202.0-blue" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2024-b7410e" />
  <img alt="Platforms" src="https://img.shields.io/badge/Platforms-macOS%20%7C%20Linux%20%7C%20Windows-555" />
</p>

## 定位与理念

`obnote` 面向需要**长期管理本地 Markdown 笔记与海量办公文档**的个人与团队：

- **告别 Electron 内存焦虑**：市面常见笔记软件基于 WebView / Electron 构建，常驻内存动辄数百 MB；`obnote` 全栈使用纯 Rust 与自研 CPU 光栅化引擎，无任何 WebView / Electron 依赖，冷启动仅约 **119 ms**、常驻内存约 **50 MB**（打开 10 万行文档约 52 MB）。
- **克制、实用的资料库体验**：不做臃肿的力导向关系图谱（Graph View），也不依赖重型插件生态，聚焦「**写得顺手 + 搜得极快 + 什么办公文档都能直接看**」。
- **纯净开箱默认**：Git、终端、SSH 远程、Vim 等研发向模块均可关闭，配置为纯净资料库模式后，打开即进入专注阅读与写作界面。

## 核心功能与格式矩阵

| 类别 | 支持能力 |
| --- | --- |
| **Markdown 实时笔记** | Live Preview（光标处自动展开源码）、LaTeX 数学公式、9 类原生 Mermaid 图表、`[[双链跳转]]`、剪贴板粘贴自动存图（支持 `assets/${filename}` 等模板）、大纲 / 目录 TOC、专注模式与打字机模式 |
| **办公与多格式预览** | Word（`.docx`）、Excel / 表格（`.xlsx` / `.csv` / `.tsv`）、PowerPoint（`.pptx` 纯 Rust 零新依赖拆页预览）、HTML 阅读模式（内存占用 < 5 MB，支持一键在浏览器中打开）、PDF（macOS / Windows 位图渲染 + 大纲 + 缩略图）、EPUB、SVG 与主流图片 |
| **资料检索与外部接力** | 项目极速全文搜索（原生支持检索 `.docx` 与 `.pptx` 正文）、20 万级文件秒开快搜（Quick Open）、预览栏一键「**用系统默认应用打开**」（方便 PDF 批注或 Office 精排编辑） |
| **界面与交互** | 16 套内置浅色 / 深色主题、完整简体中文与英文界面、触摸屏与触控板捏合缩放 |

## 基于什么开发（上游致谢）

`obnote` 基于优秀的开源项目 **[Winc](https://github.com/DEEIX-AI/Winc)**（Apache-2.0）开发。

Winc 提供了纯 Rust 桌面窗口与自研 CPU 光栅化渲染底座（macOS 下直接写入 IOSurface），带来了约 **119 ms** 冷启动、约 **50 MB** 内存占用与流畅的超大文件滚动体验。

为了能够持续、低成本地同步上游性能优化与底层修复，`obnote` 采用 **Thin Rebrand（轻量品牌定制）** 策略：
- 独立产品入口 `obnote`、独立应用图标、独立单实例锁与 URL Scheme（`obnote://`）；
- 独立配置与状态目录，可与原版 Winc 在同一台机器上共存互不干扰；
- 保留内部 `winc-*` crate 结构与模块分层，便于清晰追踪并合并上游更新。

## 快速开始

### 从源码构建与运行

需要 Rust 工具链（仓库根目录 `rust-toolchain.toml` 会自动选择 `1.99.1`）：

```bash
# 构建并直接打开你的本地知识库 / 资料目录
cargo run --release --bin obnote -- <你的资料库路径>

# 或者先编译再运行二进制
cargo build --release --bin obnote
./target/release/obnote <你的资料库路径>
```

### 配置文件路径

`obnote` 的配置文件与原版 Winc 完全隔离，各平台默认路径如下：

| 平台 | 配置文件路径 |
| --- | --- |
| **macOS** | `~/Library/Application Support/obnote/config.json` |
| **Linux** | `~/.config/obnote/config.json` |
| **Windows** | `%APPDATA%\obnote\config.json` |

### 纯净资料库模式示例配置

将以下片段写入上述 `config.json`，即可关闭 Git、终端、SSH 远程与 Vim 模式，启用中文界面、自动保存、大纲与按笔记名归档粘贴图片：

```json
{
  "language": "zh-CN",
  "theme": "auto",
  "editor": {
    "defaultLanguage": "markdown",
    "autoSave": "onFocusChange",
    "restoreFolder": true,
    "wordWrap": true,
    "outline": true,
    "lineNumbers": false
  },
  "markdown": {
    "textWidth": "standard",
    "imageFolder": "assets/${filename}",
    "imageName": "${date}_${filename}"
  },
  "git": { "enabled": false },
  "terminal": { "enabled": false },
  "remote": { "enabled": false },
  "vim": { "enabled": false }
}
```

## 路线图（Roadmap）

- **Stage 1（已完成）**：
  - [x] Thin Rebrand（独立 `obnote` 二进制、配置目录、单实例锁与全新折页书签图标）
  - [x] 预览栏一键「用系统默认应用打开」外部接力按钮
  - [x] HTML 轻量阅读模式（< 5 MB 内存占用 + 保留一键浏览器打开）
  - [x] PowerPoint（`.pptx`）纯 Rust 拆页预览
  - [x] 项目全文检索原生支持 `.docx` 与 `.pptx` 正文内容
- **Stage 2（规划中）**：
  - [ ] 反向链接（Backlinks）面板
  - [ ] `#tags` 与 YAML Frontmatter 属性索引
  - [ ] 文件重命名时自动更新 `[[双链]]` 引用
  - [ ] PDF 文本选中与复制

## 许可证

`obnote` 遵循 [Apache License, Version 2.0](LICENSE) 开源协议，并完整保留上游 [DEEIX AI / Winc](https://github.com/DEEIX-AI/Winc) 的 [LICENSE](LICENSE) 与 [NOTICE](NOTICE) 版权声明。
