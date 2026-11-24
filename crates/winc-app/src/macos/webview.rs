//! macOS 原生 `WKWebView` 宿主：仅在预览本地 `.html` 时按需挂载，关闭即释放 `WebContent` 进程。

#![allow(dead_code, reason = "待 app 预览集成层接入 WebViewHost")]

use std::path::Path;

use block2::Block;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, ClassBuilder, Sel};
use objc2::{class, msg_send, sel};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use winit::window::Window;

use super::files::{file_url, ns_string, rust_string};
use super::ns_view;

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

/// `WKNavigationActionPolicyCancel`：取消在 `WKWebView` 内继续导航。
const POLICY_CANCEL: isize = 0;

/// `WKNavigationActionPolicyAllow`：允许在 `WKWebView` 内继续导航。
const POLICY_ALLOW: isize = 1;

/// `WKNavigationTypeLinkActivated`：用户点击链接触发的导航。
const NAV_LINK_ACTIVATED: isize = 0;

/// macOS 懒加载 `WKWebView` 宿主，封装原生子视图与生命周期。
pub struct WebViewHost {
    /// 宿主窗口的内容视图（`NSView`），用于查询底部坐标系高度。
    content_view: Retained<AnyObject>,
    /// 原生 `WKWebView` 实例，由本结构独占持有。
    webview: Retained<AnyObject>,
    /// 导航委托实例（`WKWebView` 对其弱引用），随宿主一同释放。
    #[allow(dead_code, reason = "WKWebView 仅弱引用 delegate，需由宿主保持存活")]
    delegate: Option<Retained<AnyObject>>,
    /// 上一次提交给 `AppKit` 的矩形，用于跳过重复的 `setFrame:` 调用。
    last_frame: Option<CGRect>,
    /// 当前是否处于可见状态，用于跳过重复的 `setHidden:` 调用。
    visible: bool,
}

impl WebViewHost {
    /// 在 `window` 的内容视图上创建并挂载 `WKWebView`；取不到原生窗口时返回 `None`。
    pub fn new(window: &Window) -> Option<Self> {
        let content_view = ns_view(window)?;
        // SAFETY: `content_view` 是主线程上仍然存活的 `NSView`；`Retained::retain` 增加一次引用计数。
        let content_view =
            unsafe { Retained::retain(std::ptr::from_ref(content_view).cast_mut())? };

        // SAFETY: 以下均为 WebKit / AppKit 的公开 API，在主线程调用。`WKWebViewConfiguration`
        // 的 `new` 与 `WKWebView` 的 `alloc` + `initWithFrame:configuration:` 均返回 +1 对象，交给
        // `Retained` 管理；初始隐藏以避免在首次排版前闪烁空白视图。
        let webview = unsafe {
            let config: *mut AnyObject = msg_send![class!(WKWebViewConfiguration), new];
            let config = Retained::from_raw(config)?;
            let view: *mut AnyObject = msg_send![class!(WKWebView), alloc];
            let view: *mut AnyObject = msg_send![
                view,
                initWithFrame: CGRect::ZERO,
                configuration: &*config
            ];
            let view = Retained::from_raw(view)?;
            let () = msg_send![&*view, setHidden: Bool::YES];
            view
        };

        let delegate = navigation_delegate();
        if let Some(delegate) = delegate.as_ref() {
            // SAFETY: `webview` 为有效的 `WKWebView`，`delegate` 实现了 `WKNavigationDelegate`
            // 的策略回调方法，且由 `WebViewHost` 保持存活直至 `Drop` 时置空。
            unsafe {
                let () = msg_send![&*webview, setNavigationDelegate: &**delegate];
            }
        }

        // SAFETY: `content_view` 与 `webview` 均在主线程上有效，`addSubview:` 会持有子视图引用，
        // 直到 `Drop` 中调用 `removeFromSuperview`。
        unsafe {
            let () = msg_send![&*content_view, addSubview: &*webview];
        }

        Some(Self {
            content_view,
            webview,
            delegate,
            last_frame: None,
            visible: false,
        })
    }

    /// 更新预览区逻辑坐标矩形 `(x, y, w, h)`（左上角为原点）及显隐状态。
    pub fn set_frame(&mut self, rect: (f32, f32, f32, f32), visible: bool) {
        // SAFETY: `self.content_view` 为主线程上有效的 `NSView`，`bounds` 返回值类型的 `CGRect`。
        let bounds: CGRect = unsafe { msg_send![&*self.content_view, bounds] };
        let frame = appkit_frame(rect, bounds.size.height);

        if self.last_frame != Some(frame) {
            // SAFETY: `self.webview` 为主线程上有效的 `NSView` 子类，`setFrame:` 接收值类型 `CGRect`。
            unsafe {
                let () = msg_send![&*self.webview, setFrame: frame];
            }
            self.last_frame = Some(frame);
        }

        if visible != self.visible {
            // SAFETY: `self.webview` 为主线程上有效的 `NSView` 子类，`setHidden:` 接收 `Bool`。
            unsafe {
                let () = msg_send![&*self.webview, setHidden: Bool::new(!visible)];
            }
            self.visible = visible;
        }
    }

    /// 加载本机 HTML 文件（`loadFileURL:allowingReadAccessToURL:`，授权目录为其父目录或 `read_access`）。
    pub fn load_file(&mut self, path: &Path, read_access: Option<&Path>) {
        let Some(url) = file_url(path, false) else {
            return;
        };
        let access_dir = read_access.or_else(|| path.parent()).unwrap_or(path);
        let Some(access_url) = file_url(access_dir, true) else {
            return;
        };

        // SAFETY: `self.webview` 是主线程上有效的 `WKWebView`，`url` 与 `access_url` 由 `Retained`
        // 持有并保证为有效的 `file://` `NSURL`。
        unsafe {
            let _nav: *mut AnyObject = msg_send![
                &*self.webview,
                loadFileURL: &*url,
                allowingReadAccessToURL: &*access_url
            ];
        }
    }

    /// 加载内存中的 HTML 字符串（`loadHTMLString:baseURL:`，供未保存编辑或远端缓冲使用）。
    pub fn load_html(&mut self, html: &str, base_dir: Option<&Path>) {
        let Some(html_ns) = ns_string(html) else {
            return;
        };
        let base_url = base_dir.and_then(|dir| file_url(dir, true));
        let base_ptr: *const AnyObject = base_url
            .as_deref()
            .map_or(std::ptr::null(), std::ptr::from_ref);

        // SAFETY: `self.webview` 是主线程上有效的 `WKWebView`，`html_ns` 与可选的 `base_url` 在本块
        // 内保持存活；`baseURL:` 允许传空指针。
        unsafe {
            let _nav: *mut AnyObject = msg_send![
                &*self.webview,
                loadHTMLString: &*html_ns,
                baseURL: base_ptr
            ];
        }
    }
}

impl Drop for WebViewHost {
    fn drop(&mut self) {
        // SAFETY: `self.webview` 是主线程上有效的 `WKWebView`；先停止加载并清空弱引用委托，再从
        // 父视图移除，随后由 `Retained` 释放引用，确保 `WebContent` 子进程立即销毁。
        unsafe {
            let () = msg_send![&*self.webview, stopLoading];
            let nil: *mut AnyObject = std::ptr::null_mut();
            let () = msg_send![&*self.webview, setNavigationDelegate: nil];
            let () = msg_send![&*self.webview, removeFromSuperview];
        }
    }
}

/// 将左上角原点的逻辑矩形 `(x, y, w, h)` 转换为 `AppKit` 左下角原点的 `CGRect`。
pub(crate) fn appkit_frame(rect: (f32, f32, f32, f32), content_height: f64) -> CGRect {
    let (x, y, w, h) = rect;
    let width = f64::from(w.max(0.0));
    let height = f64::from(h.max(0.0));
    let x = f64::from(x);
    let y = content_height - f64::from(y) - height;
    CGRect::new(CGPoint::new(x, y), CGSize::new(width, height))
}

/// 创建 `ObnoteWebViewNavigationDelegate` 实例；注册失败时返回 `None`。
fn navigation_delegate() -> Option<Retained<AnyObject>> {
    let class = delegate_class()?;
    // SAFETY: `class` 为有效的 `NSObject` 子类，`new` 返回 +1 的实例，交给 `Retained` 管理。
    unsafe {
        let obj: *mut AnyObject = msg_send![class, new];
        Retained::from_raw(obj)
    }
}

/// 注册或获取 `ObnoteWebViewNavigationDelegate` Objective-C 类。
fn delegate_class() -> Option<&'static AnyClass> {
    if let Some(class) = AnyClass::get(c"ObnoteWebViewNavigationDelegate") {
        return Some(class);
    }
    let mut builder = ClassBuilder::new(c"ObnoteWebViewNavigationDelegate", class!(NSObject))?;
    if let Some(protocol) = AnyProtocol::get(c"WKNavigationDelegate") {
        builder.add_protocol(protocol);
    }
    type DecidePolicy =
        extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject, *mut AnyObject);
    // SAFETY: `decide_navigation_policy` 的签名与 `WKNavigationDelegate` 的
    // `webView:decidePolicyForNavigationAction:decisionHandler:` 完全一致，在 `register()` 前添加。
    unsafe {
        builder.add_method(
            sel!(webView:decidePolicyForNavigationAction:decisionHandler:),
            decide_navigation_policy as DecidePolicy,
        );
    }
    Some(builder.register())
}

/// 拦截外部 `http://` / `https://` 链接点击并交由系统默认浏览器打开，允许 `file://` 与 `about:blank`。
extern "C-unwind" fn decide_navigation_policy(
    _this: *mut AnyObject,
    _cmd: Sel,
    _web_view: *mut AnyObject,
    navigation_action: *mut AnyObject,
    decision_handler: *mut AnyObject,
) {
    let mut policy = POLICY_ALLOW;
    // SAFETY: `navigation_action` 由 WebKit 在主线程传入，方法 `request`、`navigationType`、
    // `targetFrame` 与 `NSURL` 的 `scheme` 均为公开只读属性；若为外部网页链接点击则交由
    // `NSWorkspace` 打开并取消内置跳转。
    unsafe {
        if let Some(action) = navigation_action.as_ref() {
            let request: *mut AnyObject = msg_send![action, request];
            let url: *mut AnyObject = request
                .as_ref()
                .map_or(std::ptr::null_mut(), |req| msg_send![req, URL]);
            if let Some(url_ref) = url.as_ref() {
                let scheme_obj: *mut AnyObject = msg_send![url_ref, scheme];
                let scheme = scheme_obj
                    .as_ref()
                    .and_then(|s| rust_string(s))
                    .map(|s| s.to_ascii_lowercase());
                let is_http = matches!(scheme.as_deref(), Some("http" | "https"));
                if is_http {
                    let nav_type: isize = msg_send![action, navigationType];
                    let target_frame: *mut AnyObject = msg_send![action, targetFrame];
                    if nav_type == NAV_LINK_ACTIVATED || target_frame.is_null() {
                        let workspace: *mut AnyObject =
                            msg_send![class!(NSWorkspace), sharedWorkspace];
                        if let Some(ws) = workspace.as_ref() {
                            let _: bool = msg_send![ws, openURL: url_ref];
                        }
                        policy = POLICY_CANCEL;
                    }
                }
            }
        }

        if let Some(handler) = decision_handler.cast::<Block<dyn Fn(isize)>>().as_ref() {
            handler.call((policy,));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appkit_frame_converts_top_left_to_bottom_left() {
        let frame = appkit_frame((120.0, 40.0, 640.0, 480.0), 900.0);
        assert_eq!(frame.origin.x, 120.0);
        assert_eq!(frame.origin.y, 380.0);
        assert_eq!(frame.size.width, 640.0);
        assert_eq!(frame.size.height, 480.0);
    }

    #[test]
    fn appkit_frame_clamps_negative_dimensions() {
        let frame = appkit_frame((10.0, 20.0, -50.0, -10.0), 600.0);
        assert_eq!(frame.origin.x, 10.0);
        assert_eq!(frame.origin.y, 580.0);
        assert_eq!(frame.size.width, 0.0);
        assert_eq!(frame.size.height, 0.0);
    }

    #[test]
    fn appkit_frame_full_height_panel_starts_at_zero_y() {
        let frame = appkit_frame((0.0, 0.0, 800.0, 600.0), 600.0);
        assert_eq!(frame.origin.x, 0.0);
        assert_eq!(frame.origin.y, 0.0);
        assert_eq!(frame.size.width, 800.0);
        assert_eq!(frame.size.height, 600.0);
    }
}
