# AGENTS.md — obnote 工程协作指南

本文档面向人类开发者与 AI 编码助手（Agent），约定 `obnote` 仓库的定位、架构边界、构建测试命令与工程红线。

---

## 1. 项目定位与设计原则

`obnote` 是基于 [Winc](https://github.com/DEEIX-AI/Winc)（Apache-2.0）演进而来的**纯 Rust、零 WebView 本地个人知识库与办公资料管理器（Obsidian-lite）**。

- **核心工作流**：以本地文件夹为 Vault，聚焦日常写作、阅读与资料检索：
  - **Markdown 笔记**：Live Preview（光标处展开源码）、LaTeX 公式、9 类原生 Mermaid 图表、`[[双链跳转]]`、剪贴板自动存图（`assets/${filename}`）、大纲 TOC、专注/打字机模式。
  - **办公与多格式资料预览**：Word (`.docx`)、Excel (`.xlsx` / `.csv` / `.tsv`)、PowerPoint (`.pptx` 拆页阅读视图)、HTML 阅读模式（`<5 MB` 内存 + 一键浏览器打开）、PDF（macOS/Windows 位图渲染 + 大纲 + 缩略图）、EPUB、SVG 与主流位图。
  - **全文检索与外部接力**：项目极速搜索（原生支持检索 `.docx` 与 `.pptx` 正文）、20 万文件级快搜、预览栏一键「用系统默认应用打开」（方便 PDF 批注或 Office 精排编辑）。
- **明确的非目标（Non-goals）**：
  - 不做 Force-directed Graph View（关系图谱）。
  - 不引入 Electron / Chromium（窄例外：macOS `.html` 预览/分栏视图采用按需懒加载的系统 `WKWebView`，未打开时 0 MB、切走或关闭后立即释放 `WebContent` 进程）。
  - 不构建重型 JS/WASM 插件运行时。

---

## 2. 薄改名 (Thin Rebrand) 与上游同步红线

为保证长期低成本同步上游改进，`obnote` 严格采用**薄改名（Thin Rebrand）**策略：

1. **严禁重命名内部 `winc-*` Crate**：
   - `crates/winc-core`、`winc-editor`、`winc-preview`、`winc-render`、`winc-app`、`winc-config`、`winc-typeset`、`winc-backend` 等内部 crate 名称**保持不变**。
   - 仅对外品牌、二进制名（`obnote`）、配置目录（`obnote`）、单实例锁（`obnote`）、URL Scheme（`obnote://`）、图标与应用元数据做改名隔离。
2. **上游只读同步**：
   - `upstream` 远端固定指向 `https://github.com/DEEIX-AI/Winc.git`，保持只读（严禁向 `upstream` 推送）。
   - 同步上游时按 `upstream/dev -> main` 合并，严禁无故大面积格式化或重构存量 `winc-*` 模块，避免制造合并冲突。
3. **未来知识库能力隔离**：
   - 阶段 2/3 的知识库专属功能（反向链接 Backlinks、`#tags` / Frontmatter 索引、文件重命名自动更新双链等）**必须**新建独立 crate `crates/winc-kb`，在 `winc-app` 中仅保留极薄的胶水调用。

---

## 3. 目录与 Crate 分层速查

### 核心 Crate 分层

| Crate | 职责 | 关键依赖关系 |
| :--- | :--- | :--- |
| `crates/winc` | 二进制入口（产出 `[[bin]] name = "obnote"`） | 仅依赖 `winc-app` |
| `crates/winc-app` | 窗口生命周期、布局、事件分发、UI 绘制与视图状态 | 依赖 `winc-editor`、`winc-preview`、`winc-render`、`winc-config` 等 |
| `crates/winc-editor` | 文档模型、Rope 文本存储、磁盘读写、Git、项目全文搜索 | 依赖 `winc-core`、`winc-preview`、`winc-backend` |
| `crates/winc-preview` | Markdown/HTML/DOCX/PPTX/XLSX/EPUB/PDF/SVG/图片解析与转换 | 纯数据解析层，不依赖 `winc-app` |
| `crates/winc-render` | 纯 CPU 逐像素光栅化合成器与字体排版渲染 | 被 `winc-app` 调用 |
| `crates/winc-typeset` | 段落排版、行内样式、LaTeX/Mermaid 布局 | 被 `winc-app` / `winc-preview` 使用 |
| `crates/winc-config` | 配置读写、状态持久化、原子落盘与便携模式目录解析 | 基础配置层 |
| `crates/winc-i18n` | Fluent 多语言文案（`en-US` / `zh-CN`） | 被 `winc-app` 使用 |
| `crates/winc-kb` *(规划)* | 反向链接、标签/Frontmatter 索引、重命名更新双链 | 独立隔离 crate（待建） |
| `xtask` | 开发自动化（`bench`、`test`、`icons`、`app`、`dmg`） | 仅开发/打包使用 |

### `obnote` 定制文件落点

- **二进制与版本**：`crates/winc/Cargo.toml`（`[[bin]] name = "obnote"`）、`VERSION`
- **配置与状态目录**：`crates/winc-config/src/files.rs`、`crates/winc-config/src/state.rs`
- **品牌、进程锁与 URL Scheme**：`crates/winc-app/src/instance.rs`、`crates/winc-app/src/cli.rs`、`crates/winc-app/src/layout/settings/about.rs`、`crates/winc-app/src/app/lifecycle/events.rs`、`crates/winc-app/src/lib.rs`
- **图标与启动动画**：`crates/winc-app/assets/icon.svg`、`crates/winc-app/assets/winc.ico`、`crates/winc-app/src/app/setup.rs`
- **预览扩展与全文搜索**：`crates/winc-preview/src/pptx.rs`、`crates/winc-preview/src/zip.rs`、`crates/winc-editor/src/document.rs`、`crates/winc-editor/src/document/disk.rs`、`crates/winc-editor/src/workspace/project_search/scan.rs`、`crates/winc-app/src/app/preview.rs`、`crates/winc-app/src/app/preview/markdown.rs`
- **打包与发布配置**：`release/common/config.mjs`、`release/macos/package.sh`、`xtask/src/dmg.rs`、`xtask/src/bench.rs`
- **项目文档**：`README.md`、`README.zh-CN.md`、`AGENTS.md`、`docs/index.md`、`docs/architecture/obnote-blueprint.md`

---

## 4. Rust 编译、测试与发布规范

> **目标平台红线**：本地开发与构建 **仅面向 macOS Apple Silicon (ARM64 / `aarch64-apple-darwin`)**，不考虑 Windows、Linux 或 Intel macOS (`x86_64`) 的跨平台适配与打包。

### 本地 macOS ARM64 `.app` 打包

- 首次全新构建需先编译 PTY 助手：`cargo build --release --locked -p winc-pty`
- 本地一键构建并打包 `.app`（产物：`target/release/bundle/obnote.app`）：

```bash
cargo build --release --bin obnote && cargo xtask app --no-build
```

### 工具链与 Profile 说明

- **Toolchain**（见 `rust-toolchain.toml`）：`channel = "1.99.1"`，`profile = "minimal"`，`components = ["rustfmt", "clippy"]`。
- **Edition & MSRV**（见 `Cargo.toml`）：`edition = "2024"`，`rust-version = "1.89"`。
- **Dev Profile**：`debug = "line-tables-only"`；所有第三方依赖与 `winc-render` 开启 `opt-level = 2`，保证本地交互与光栅化流畅，同时保持自有 crate 增量编译速度。
- **Release Profile**：`opt-level = 2`、`lto = "fat"`、`codegen-units = 1`、`strip = true`、`panic = "abort"`（产物 `target/release/obnote` 约 **12 MB**）。`winc-app` 与通用依赖按 `opt-level = "s"` 控体积，热路径 crate（`memchr`、`regex`、`fancy-regex`、`ropey`、`png`、`zune-jpeg`、`image-webp`、`encoding_rs` 等）保持 `opt-level = 2`。

### 常用命令

```bash
# 1. 以 Release 模式运行并打开指定知识库目录
cargo run --release --bin obnote -- <目录>

# 2. 构建 Release 二进制（产物：target/release/obnote）
cargo build --release --bin obnote

# 3. 工作区静态检查（零警告门禁）
cargo clippy --workspace --all-targets -- -D warnings

# 4. 文档链接与缺失文档检查
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# 5. 核心逻辑 crate 单元测试
cargo test -p winc-config -p winc-preview -p winc-editor -- --test-threads=2

# 6. 应用层快照与交互测试
#    说明：
#    - 并行 CPU 抢占可能导致 latex 相关测试偶发超时，建议限制 --test-threads=2（或 1）
#    - 若本机 ~/.gitconfig 全局设置了 pull.rebase=true，请加 GIT_CONFIG_GLOBAL=/dev/null
#      以确保 git 同步/弹窗快照测试行为确定
GIT_CONFIG_GLOBAL=/dev/null cargo test -p winc-app -- --test-threads=2

# 7. 图标校验与重新生成
cargo test -p xtask icons
cargo xtask icons <输出目录>

# 8. 性能基准测试（默认 20 轮）
#    硬性预算（xtask/src/bench.rs）：
#    - 空载物理内存 <= 50 MB
#    - 冷启动首帧 <= 200 ms
#    - 代码区滚动 p95 <= 2 ms
#    - 按键响应 p95 <= 4 ms
#    - Markdown 预览滚动 p95 <= 3 ms
cargo xtask bench
```

---

## 5. 工程红线与代码规范

1. **Workspace Lints（见 `Cargo.toml` `[workspace.lints]`）**：
   - `unsafe_code = "deny"`：全工作区禁止 `unsafe` 代码。
   - `missing_docs = "warn"`：所有公开模块、结构体、枚举、函数必须编写清晰的中文 rustdoc。
   - `broken_intra_doc_links = "warn"` / `private_intra_doc_links = "warn"`：doc 内链接必须有效且可见。
   - Clippy 开启 `dbg_macro`、`todo`、`missing_errors_doc`、`missing_panics_doc`、`doc_markdown` 等检查，提交前必须零警告通过。
2. **内存与架构红线**：
   - 严禁引入 Electron / Chromium；系统 `WKWebView` 仅作为 macOS `.html` 预览/分栏的窄例外按需懒加载（未打开时 0 MB，切走或关闭即销毁）。
   - 保持空载物理内存 `<= 50 MB`、二进制体积紧凑。
   - 新增文件格式预览或索引功能优先复用 `winc-preview` 现有基础设施（如 `zip::Archive`、`ooxml`、`roxmltree`、`from_html`），避免引入重型依赖。
3. **开源协议合规**：
   - 严禁删除或篡改根目录 `LICENSE` 与 `NOTICE`（Apache-2.0 对 DEEIX AI / Winc 的署名）。
4. **配置与状态目录**：
   - **macOS**：`~/Library/Application Support/obnote/config.json`（及同目录下 `state.json`）
   - **Linux**：`$XDG_CONFIG_HOME/obnote/config.json` 或 `~/.config/obnote/config.json`
   - **Windows**：`%APPDATA%\obnote\config.json`（若 `obnote.exe` 同级存在 `data/` 目录则启用便携模式，配置与状态均落盘至 `data/`）
