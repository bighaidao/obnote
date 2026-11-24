//! macOS `.html` 预览的原生 `WKWebView` 懒加载管理：按需创建、智能重载、浮层遮挡隐藏与即时销毁。
//!
//! 仅在 macOS 且存在真实窗口句柄时接管 `PreviewKind::Html` 的预览与分栏视图；
//! 切回源码、切换非 HTML 标签、关闭标签或出现错误占位页时立即释放 `WebViewHost`，
//! 使 `WebContent` 子进程随之退出，保持空载零内存开销。

use std::path::{Path, PathBuf};

use winc_editor::{Document, DocumentId, PreviewKind, PreviewMode};

use crate::app::App;

/// 触发 `WebView` 重新加载内容的键；仅当文档、版本号、路径或脏标记变化时重新加载。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReloadKey {
    /// 文档标识。
    pub(crate) doc: DocumentId,
    /// 文档版本号。
    pub(crate) revision: u64,
    /// 文档路径（未保存时为 `None`）。
    pub(crate) path: Option<PathBuf>,
    /// 是否有未保存的修改。
    pub(crate) dirty: bool,
}

impl ReloadKey {
    /// 从当前文档提取重载键。
    pub(crate) fn from_doc(doc: &Document) -> Self {
        Self {
            doc: doc.id,
            revision: doc.revision,
            path: doc.path.clone(),
            dirty: doc.is_dirty(),
        }
    }
}

/// 当前帧应当展示的 `WebView` 目标区域与文档信息。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WebViewTarget {
    /// 逻辑像素矩形 `(x, y, w, h)`。
    pub(crate) rect: (f32, f32, f32, f32),
    /// 是否可见（区域宽高大于 0 且无任何 CPU 浮层/弹窗遮挡）。
    pub(crate) visible: bool,
    /// 用于判断是否需要重新加载内容的键。
    pub(crate) reload: ReloadKey,
}

/// `WebView` 内容加载来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoadSource {
    /// 本机磁盘上未修改的文件：直接加载文件并授予工作区或父目录读权限，以支持相对路径资源。
    File {
        path: PathBuf,
        read_access: Option<PathBuf>,
    },
    /// 有未保存修改、未命名或远端文件：直接加载内存中的 HTML 字符串。
    Html {
        html: String,
        base_dir: Option<PathBuf>,
    },
}

/// 懒加载的原生 `WebView` 预览状态（仅 macOS `.html` 预览/分栏时存在）。
pub(crate) struct WebViewState {
    #[cfg(target_os = "macos")]
    host: crate::macos::WebViewHost,
    loaded: Option<ReloadKey>,
    last_frame: Option<((f32, f32, f32, f32), bool)>,
}

impl WebViewState {
    /// 创建原生 `WebView` 状态。
    #[cfg(target_os = "macos")]
    fn new(host: crate::macos::WebViewHost) -> Self {
        Self {
            host,
            loaded: None,
            last_frame: None,
        }
    }
}

impl App {
    /// 当前是否由原生 `WKWebView` 接管此文档的 HTML 预览。
    ///
    /// 仅在 macOS 且存在真实窗口句柄（非无头测试）时接管；无头测试或非 macOS 平台仍走
    /// CPU Markdown 解析与绘制。
    pub(crate) fn uses_webview(&self, doc: &Document) -> bool {
        cfg!(target_os = "macos")
            && self.window.handle.is_some()
            && doc.preview_kind() == Some(PreviewKind::Html)
            && doc.unsupported.is_none()
            && doc.reading.is_none()
    }

    /// 当前是否有任何由 CPU 绘制、可能遮挡编辑器/预览区的浮层、弹窗或模态框。
    ///
    /// 原生 `WKWebView` 作为 `NSView` 浮在 CPU 像素缓冲之上；任一浮层打开时必须隐藏
    /// `WKWebView`，避免原生视图盖住菜单、搜索条、快速打开卡片或设置弹窗。
    pub(crate) fn has_overlay(&self) -> bool {
        self.window.hidden
            || self.settings.open
            || self.settings.motion.is_some()
            || self.overlays.form_dialog.is_some()
            || self.motion.dialog_card.motion.is_some()
            || self.overlays.alert.is_some()
            || self.popup().is_some()
            || self.overlays.context_menu.is_some()
            || self.overlays.picker.open
            || self.overlays.picker.motion.is_some()
            || self.overlays.picker.places_in.is_some()
            || self.overlays.picker.places_out.is_some()
            || self.overlays.picker.card.motion.is_some()
            || self.bars.find.open
            || self.layout.search.h > 0.0
            || self.bars.goto_input.is_some()
            || self.motion.bar_card.motion.is_some()
            || self.search_dialog.open
            || self.motion.search_card.motion.is_some()
            || self.notice.text.is_some()
            || self.notice.leaving.is_some()
            || self.jumps.name_hint.is_some()
            || self.preview.markdown.suggest.at.is_some()
            || self.git.changes.popup.is_some()
            || self.panes.drop.is_some()
            || self.terminal.grow.is_some()
            || self.tree_dragging().is_some()
            || !self.tabs_dragged_out().is_empty()
            || self.file_hover().is_some()
    }

    /// 计算当前帧应当展示的 `WebView` 目标区域与重载键；不需要 `WebView` 时返回 `None`。
    pub(crate) fn webview_target(&self) -> Option<WebViewTarget> {
        let doc = self.editor.active()?;
        if doc.preview_kind() != Some(PreviewKind::Html)
            || doc.unsupported.is_some()
            || doc.reading.is_some()
        {
            return None;
        }
        let area = match doc.preview_mode() {
            PreviewMode::Preview => self.layout.editor,
            PreviewMode::Split if self.layout.split.w > 0.0 && self.layout.split.h > 0.0 => {
                self.layout.split
            }
            PreviewMode::Source | PreviewMode::Split => return None,
        };
        let rect = (area.x, area.y, area.w, area.h);
        let visible = area.w > 0.0 && area.h > 0.0 && !self.has_overlay();
        Some(WebViewTarget {
            rect,
            visible,
            reload: ReloadKey::from_doc(doc),
        })
    }

    /// 为当前激活的 HTML 文档决定加载来源（磁盘文件或内存 HTML 文本）。
    pub(crate) fn webview_load_source(&self, doc: &Document) -> Option<LoadSource> {
        if let Some(path) = doc
            .path
            .as_deref()
            .filter(|p| !doc.is_dirty() && !winc_backend::is_remote(p))
        {
            let read_access = self.webview_read_access(path).map(Path::to_path_buf);
            return Some(LoadSource::File {
                path: path.to_path_buf(),
                read_access,
            });
        }
        let rope = doc.text().rope()?;
        let base_dir = doc
            .path
            .as_deref()
            .filter(|p| !winc_backend::is_remote(p))
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        Some(LoadSource::Html {
            html: rope.to_string(),
            base_dir,
        })
    }

    /// 计算本地 HTML 文件加载时的授权读取根目录：优先工作区根目录（若文件位于其中），否则取父目录。
    fn webview_read_access<'a>(&'a self, path: &'a Path) -> Option<&'a Path> {
        if let Some(root) = self.editor.workspace.as_ref().map(|w| w.root.as_path())
            && path.starts_with(root)
        {
            return Some(root);
        }
        path.parent()
    }

    /// 立即释放不再需要的 `WebView` 实例（退出、关闭标签或切换模式时调用）。
    pub(crate) fn drop_webview(&mut self) {
        self.preview.webview = None;
    }

    /// 同步原生 `WKWebView` 的生命周期、位置、可见性与内容。
    ///
    /// - 非 macOS 或无窗口句柄时不创建宿主，若已有状态则释放；
    /// - 当前标签不是 HTML 预览/分栏（或处于错误/不支持占位页）时立即释放；
    /// - 仅当 `ReloadKey` 变化时重新加载，仅当位置或可见性变化时更新 `set_frame`。
    pub(crate) fn sync_webview(&mut self) {
        #[cfg(not(target_os = "macos"))]
        {
            self.preview.webview = None;
        }
        #[cfg(target_os = "macos")]
        {
            let Some(window) = self.window.handle.as_ref() else {
                self.preview.webview = None;
                return;
            };
            let Some(target) = self.webview_target() else {
                self.preview.webview = None;
                return;
            };
            if self.preview.webview.is_none() {
                let Some(host) = crate::macos::WebViewHost::new(window) else {
                    return;
                };
                self.preview.webview = Some(WebViewState::new(host));
            }
            let needs_reload = self
                .preview
                .webview
                .as_ref()
                .is_some_and(|wv| wv.loaded.as_ref() != Some(&target.reload));
            let source = if needs_reload {
                self.editor
                    .active()
                    .and_then(|doc| self.webview_load_source(doc))
            } else {
                None
            };
            let Some(wv) = self.preview.webview.as_mut() else {
                return;
            };
            if let Some(source) = source {
                match source {
                    LoadSource::File { path, read_access } => {
                        wv.host.load_file(&path, read_access.as_deref());
                    }
                    LoadSource::Html { html, base_dir } => {
                        wv.host.load_html(&html, base_dir.as_deref());
                    }
                }
                wv.loaded = Some(target.reload);
            }
            let next_frame = (target.rect, target.visible);
            if wv.last_frame != Some(next_frame) {
                wv.host.set_frame(target.rect, target.visible);
                wv.last_frame = Some(next_frame);
            }
        }
    }
}
