# obnote 架构蓝图

## 1. 核心架构与渲染管线

`obnote` 基于纯 Rust 与自研 CPU 逐像素光栅化合成器（`winc-render`），保持冷启动 `<= 200 ms`、空载物理内存 `<= 50 MB` 的轻量预算。

- **文档与工作区层（`winc-editor`）**：Rope 文本存储、异步磁盘 I/O、Git 集成与项目全文搜索。
- **解析与转换层（`winc-preview`）**：Markdown、LaTeX、Mermaid、DOCX、PPTX、XLSX、EPUB、PDF、SVG 与位图解析。
- **应用与视图层（`winc-app`）**：窗口生命周期、布局、事件分发与 UI 绘制。

## 2. 窄例外：macOS `.html` 预览的懒加载 `WKWebView`

为保证复杂 `.html` 文档（完整 CSS、相对资源与交互页面）的还原度，同时守住空载内存与冷启动预算，macOS 端对 `.html` 预览采用极窄的系统 `WKWebView` 例外策略：

1. **零空载成本（Lazy Lifecycle）**：
   - `PreviewState::webview` 初始为 `None`，冷启动与日常编辑 Markdown/代码时不创建 `WKWebView`，`WebContent` 子进程占用为 **0 MB**。
   - 仅当平台为 macOS、存在真实窗口句柄、激活标签为 `PreviewKind::Html`（且 `unsupported.is_none()`），并处于 `PreviewMode::Preview` 或 `PreviewMode::Split` 时，才调用 `WebViewHost::new(window)` 创建宿主。
   - 一旦切换到非 HTML 标签、切回 `PreviewMode::Source`、关闭标签或进入错误占位页，立即置 `self.preview.webview = None`，通过 `Drop` 从父视图移除并销毁 `WKWebView` 与 `WebContent` 进程。
2. **智能重载（Smart Reloading）**：
   - 按 `(DocumentId, revision, path, dirty)` 跟踪重载键，仅在文档或内容变化时触发加载。
   - 本地未修改文件调用 `load_file`（授予工作区根目录或父目录读权限以加载相对 CSS/JS/图片）；有未保存修改、未命名或远端文件调用 `load_html` 注入内存文本。
3. **CPU 浮层遮挡保护（Overlay Guard）**：
   - 因原生 `WKWebView` 浮于 CPU 像素缓冲之上，任一 CPU 浮层/弹窗（设置、快速打开、查找条、跳转条、右键菜单、对话框、提示框、拖拽卡片等）打开时，自动将 `WKWebView` 设为隐藏（`visible = false`），避免原生视图遮挡 UI。
   - 当原生 `WKWebView` 接管 `.html` 预览时，跳过后台 `from_document` Markdown 解析与 CPU 绘制，避免重复开销。
