//! 把剪贴板中的 HTML 转换为 Markdown：按元素语义转换，不应用样式表。
//!
//! 容忍不配对的标签、不关闭的 `<br>`、`<img>` 和不加引号的属性；未识别的元素只保留文字，
//! `script`、`style`、`head` 等元素的内容不输出。

use crate::convert::destination;

/// 转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    /// Markdown 原文。
    pub text: String,
    /// 是否含有文字以外的结构（标题、列表、链接、强调、表格、代码、图片、引用）。
    pub rich: bool,
}

/// 把 HTML `html` 转换为 Markdown（剪贴板模式）。
pub fn from_html(html: &str) -> Converted {
    convert(html, false)
}

/// 把完整的 HTML 文档 `html` 转换为适合阅读预览的 Markdown。
///
/// 与剪贴板模式 [`from_html`] 的区别：
/// - 正文没有 `<h1>` 时保留 `<title>` 作为一级标题；
/// - 过滤 `<nav>`、页脚 `<footer>` 与侧栏 `<aside>` 等页面噪音（正文内的 `<aside>` 转为引用，引用内的 `<footer>` 保留）；
/// - 保留相对路径的图片和链接（支持 `<base href>` 解析），以及 `#锚点` 链接；
/// - 优化 `<figure>`/`<figcaption>`、`<details>`/`<summary>`、`<dl>`/`<dt>`/`<dd>`、`<mark>`、`<sup>`/`<sub>`、`<caption>` 等语义标签的排版。
pub fn from_document(html: &str) -> Converted {
    convert(html, true)
}

fn convert(html: &str, document: bool) -> Converted {
    let mut w = Writer {
        document,
        ..Writer::default()
    };
    for token in Tokens(html) {
        match token {
            Token::Open(name, attrs) => w.open(&name, attrs),
            Token::Close(name) => w.close(&name),
            Token::Text(text) => w.text(&super::html::decode_entities(text)),
        }
    }
    w.finish()
}

/// HTML 中的一项：开始标签（小写名字与属性原文）、结束标签或文字（实体未解码）。
enum Token<'a> {
    Open(String, &'a str),
    Close(String),
    Text(&'a str),
}

/// 依次读出 HTML 中的各项；跳过注释、文档类型声明和处理指令，不构成标签的 `<` 作为文字。
struct Tokens<'a>(&'a str);

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            let rest = self.0;
            if rest.is_empty() {
                return None;
            }
            let Some(at) = rest.find('<') else {
                self.0 = "";
                return Some(Token::Text(rest));
            };
            if at > 0 {
                self.0 = &rest[at..];
                return Some(Token::Text(&rest[..at]));
            }
            if let Some(comment) = rest.strip_prefix("<!--") {
                self.0 = comment.find("-->").map_or("", |end| &comment[end + 3..]);
                continue;
            }
            let close = rest.starts_with("</");
            let body = &rest[1 + usize::from(close)..];
            let name_len = body
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .unwrap_or(body.len());
            if body.starts_with(['!', '?']) {
                self.0 = body.find('>').map_or("", |end| &body[end + 1..]);
                continue;
            }
            if name_len == 0 || !body.starts_with(|c: char| c.is_ascii_alphabetic()) {
                self.0 = &rest[1..];
                return Some(Token::Text("<"));
            }
            let end = tag_end(body).unwrap_or(body.len());
            self.0 = body.get(end + 1..).unwrap_or("");
            let name = body[..name_len].to_ascii_lowercase();
            return Some(match close {
                true => Token::Close(name),
                false => Token::Open(name, &body[name_len..end]),
            });
        }
    }
}

/// 标签里引号之外的第一个 `>` 的位置。
fn tag_end(body: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in body.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// 返回属性 `key` 的值（实体已解码）。
fn attr(attrs: &str, key: &str) -> Option<String> {
    super::html::attribute(&format!(" {attrs}"), key)
}

/// 包含后续内容的块：引用，或列表中的一项（续行缩进 `indent` 个空格）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Frame {
    Quote,
    List { ordered: bool, next: u32 },
    Item { indent: usize },
}

/// 行内的强调、代码或链接。
#[derive(Debug, Clone, PartialEq)]
enum Span {
    /// 强调：结束标签名和标记（`**`、`*`、`~~`）。
    Mark(String, &'static str),
    /// 保留原生行内 HTML 标签（`<mark>`、`<sup>`、`<sub>`）。
    Html(&'static str),
    Code,
    Link(String),
    /// 不改变样式的标签（如 `<b style="font-weight:normal">`），仅用于配对。
    Plain(String),
}

#[derive(Default)]
struct Writer {
    document: bool,
    out: String,
    rich: bool,
    stack: Vec<Frame>,
    /// 列表项第一块的记号，写出后清空。
    marker: Option<String>,
    /// 上一块是否位于列表项中：是时下一项不加空行。
    after_item: bool,
    /// 上一块续行的前缀；块之间的空行只保留两块共有的前缀。
    last_prefix: String,
    /// 尚未写出的行内文字。
    inline: String,
    spans: Vec<(Span, usize)>,
    heading: Option<usize>,
    /// 预排版文字的内容和语言。
    pre: Option<(String, String)>,
    table: Option<Table>,
    /// 位于内容不输出的元素中的层数。
    skip: usize,
    /// 文档模式下位于 `<head>` 中的层数。
    in_head: usize,
    /// 文档模式下是否正在读取 `<title>`。
    in_title: bool,
    /// 文档模式下收集到的 `<title>` 原文。
    title: String,
    /// 文档模式下是否已出现 `<h1>`。
    has_h1: bool,
    /// 文档模式下 `<base href>` 指定的基准地址。
    base_href: Option<String>,
    /// 文档模式下位于 `<main>` 或 `<article>` 中的层数。
    in_main_article: usize,
    /// 文档模式下是否位于 `<summary>` 中。
    in_summary: bool,
    /// 文档模式下是否位于 `<dt>` 中。
    in_dt: bool,
    /// 文档模式下是否位于 `<figcaption>` 中（斜体块）。
    in_figcaption: bool,
}

#[derive(Default)]
struct Table {
    rows: Vec<Vec<String>>,
    /// 当前单元格的内容在行内缓冲中的开始位置。
    cell: Option<usize>,
    /// 当前 `<caption>` 在行内缓冲中的开始位置。
    caption: Option<usize>,
    /// 嵌套表格的层数：嵌套的表格只保留文字。
    nested: usize,
}

impl Writer {
    fn in_quote(&self) -> bool {
        self.stack.contains(&Frame::Quote)
    }

    fn should_skip(&self, name: &str) -> bool {
        if matches!(name, "script" | "style" | "template") {
            return true;
        }
        if !self.document {
            return matches!(name, "head" | "title");
        }
        matches!(name, "noscript" | "svg" | "nav")
            || (name == "aside" && self.in_main_article == 0)
            || (name == "footer" && !self.in_quote())
    }

    fn resolve_url(&self, url: &str) -> Option<String> {
        if self.document {
            resolve_doc_url(url, self.base_href.as_deref())
        } else {
            absolute(url).then(|| url.to_owned())
        }
    }

    fn open(&mut self, name: &str, attrs: &str) {
        // 只按同类的开始、结束标签计层数，其中的其他标签不计入。
        if self.should_skip(name) {
            self.skip += 1;
            return;
        }
        if self.skip > 0 {
            return;
        }
        if self.document {
            if name == "head" {
                self.in_head += 1;
                return;
            }
            if self.in_head > 0 {
                if name == "title" {
                    self.in_title = true;
                } else if name == "base" && self.base_href.is_none() {
                    self.base_href = attr(attrs, "href");
                }
                return;
            }
            if matches!(name, "main" | "article") {
                self.in_main_article += 1;
            }
        }
        if let Some((text, lang)) = self.pre.as_mut() {
            match name {
                "br" => text.push('\n'),
                "code" if lang.is_empty() => *lang = code_lang(attrs),
                _ => {}
            }
            return;
        }
        if let Some(table) = self.table.as_mut() {
            match name {
                "table" => table.nested += 1,
                "caption" if self.document && table.nested == 0 => {
                    let at = self.inline.len();
                    if let Some(table) = self.table.as_mut() {
                        table.caption = Some(at);
                    }
                    return;
                }
                "td" | "th" if table.nested == 0 => {
                    let at = self.inline.len();
                    if let Some(table) = self.table.as_mut() {
                        table.cell = Some(at);
                    }
                    return;
                }
                "br" => {
                    self.inline.push_str("<br>");
                    return;
                }
                // 单元格中的块元素只作为分隔，内容合并为一行。
                _ if is_block(name) => {
                    self.inline.push(' ');
                    return;
                }
                _ => {}
            }
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                self.heading = Some(usize::from(name.as_bytes()[1] - b'0'));
            }
            "details" if self.document => {
                self.flush();
                let open = attr(attrs, "open").is_some() || has_flag(attrs, "open");
                self.emit(if open { "<details open>" } else { "<details>" });
                self.rich = true;
            }
            "summary" if self.document => {
                self.flush();
                self.in_summary = true;
            }
            "figcaption" if self.document => {
                self.flush();
                self.in_figcaption = true;
            }
            "dt" if self.document => {
                self.flush();
                self.in_dt = true;
            }
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "figure"
            | "figcaption" | "dl" | "dt" | "dd" | "address" | "center" | "nav" | "aside" => {
                if self.document && matches!(name, "dd" | "aside") {
                    self.flush();
                    self.stack.push(Frame::Quote);
                } else {
                    self.flush();
                }
            }
            "blockquote" => {
                self.flush();
                self.stack.push(Frame::Quote);
            }
            "ul" | "ol" => {
                self.flush();
                // 有序列表的序号最多 9 位。
                let next = attr(attrs, "start")
                    .and_then(|s| s.trim().parse().ok())
                    .filter(|&n| n <= 999_999_999)
                    .unwrap_or(1);
                let ordered = name == "ol";
                self.stack.push(Frame::List { ordered, next });
            }
            "li" => {
                self.flush();
                self.end_item();
                let list = self.stack.iter_mut().rev().find_map(|f| match f {
                    Frame::List { ordered, next } => Some((ordered, next)),
                    _ => None,
                });
                let marker = match list {
                    Some((true, next)) => {
                        let number = *next;
                        *next = (number + 1).min(999_999_999);
                        format!("{number}. ")
                    }
                    _ => "- ".to_owned(),
                };
                self.stack.push(Frame::Item {
                    indent: marker.len(),
                });
                self.marker = Some(marker);
            }
            "pre" => {
                self.flush();
                self.pre = Some((String::new(), code_lang(attrs)));
            }
            "table" => {
                self.flush();
                self.table = Some(Table::default());
            }
            "hr" => {
                self.flush();
                self.emit("---");
                self.rich = true;
            }
            "br" => self.inline.push_str("\\\n"),
            "img" => {
                let alt = attr(attrs, "alt").unwrap_or_default();
                let raw_src = attr(attrs, "src")
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| self.document.then(|| attr(attrs, "data-src")).flatten())
                    .or_else(|| {
                        self.document
                            .then(|| attr(attrs, "srcset").and_then(|s| first_srcset(&s)))
                            .flatten()
                    });
                match raw_src.and_then(|s| self.resolve_url(&s)) {
                    Some(src) => {
                        let alt = escape(alt.trim());
                        self.inline
                            .push_str(&format!("![{alt}]({})", destination(&src)));
                        self.rich = true;
                    }
                    None => self.inline.push_str(&escape(&alt)),
                }
            }
            "mark" | "sup" | "sub" if self.document => {
                let tag = match name {
                    "mark" => "mark",
                    "sup" => "sup",
                    _ => "sub",
                };
                let at = self.inline.len();
                self.spans.push((Span::Html(tag), at));
            }
            "input" if attrs.to_ascii_lowercase().contains("checkbox") => {
                let checked = attrs.to_ascii_lowercase().contains("checked");
                self.inline.push_str(if checked { "[x] " } else { "[ ] " });
            }
            "a" => {
                let href = attr(attrs, "href").unwrap_or_default();
                let at = self.inline.len();
                self.spans.push((Span::Link(href), at));
            }
            "code" | "kbd" | "samp" | "tt" => {
                let at = self.inline.len();
                self.spans.push((Span::Code, at));
            }
            "strong" | "b" | "em" | "i" | "del" | "s" | "strike" | "span" => {
                let style = attr(attrs, "style")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let style = style.replace(' ', "");
                let normal =
                    style.contains("font-weight:normal") || style.contains("font-weight:400");
                let bold = ["font-weight:bold", "font-weight:600", "font-weight:700"];
                let mark = match name {
                    "strong" | "b" if normal => None,
                    "strong" | "b" => Some("**"),
                    "em" | "i" => Some("*"),
                    "del" | "s" | "strike" => Some("~~"),
                    _ if bold.iter().any(|b| style.contains(b)) => Some("**"),
                    _ if style.contains("font-style:italic") => Some("*"),
                    _ => None,
                };
                let at = self.inline.len();
                let span = match mark {
                    Some(mark) => Span::Mark(name.to_owned(), mark),
                    None => Span::Plain(name.to_owned()),
                };
                self.spans.push((span, at));
                if let Some(mark) = mark {
                    self.inline.push_str(mark);
                }
            }
            _ => {}
        }
    }

    fn close(&mut self, name: &str) {
        if self.skip > 0 {
            if self.should_skip(name) {
                self.skip -= 1;
            }
            return;
        }
        if self.document {
            if name == "head" {
                self.in_head = self.in_head.saturating_sub(1);
                self.in_title = false;
                return;
            }
            if self.in_head > 0 {
                if name == "title" {
                    self.in_title = false;
                }
                return;
            }
            if matches!(name, "main" | "article") {
                self.in_main_article = self.in_main_article.saturating_sub(1);
            }
        }
        if name == "pre"
            && let Some((text, lang)) = self.pre.take()
        {
            let text = text.trim_matches('\n');
            let fence = "`".repeat(longest_run(text, '`').max(2) + 1);
            self.emit(&format!("{fence}{lang}\n{text}\n{fence}"));
            self.rich = true;
            return;
        }
        if self.pre.is_some() {
            return;
        }
        if let Some(table) = self.table.as_mut() {
            match name {
                "table" if table.nested > 0 => table.nested -= 1,
                "caption" if self.document && table.nested == 0 => {
                    self.close_spans(0);
                    if let Some(at) = self.table.as_mut().and_then(|t| t.caption.take()) {
                        let cap = self.inline.split_off(at.min(self.inline.len()));
                        let cap = cap.trim();
                        if !cap.is_empty() {
                            self.emit(&format!("*{cap}*"));
                            self.rich = true;
                        }
                    }
                    return;
                }
                "td" | "th" if table.nested == 0 => {
                    self.close_spans(0);
                    let Some(table) = self.table.as_mut() else {
                        return;
                    };
                    if let Some(at) = table.cell.take() {
                        let cell = self.inline.split_off(at.min(self.inline.len()));
                        let cell = cell.trim().replace('|', "\\|").replace("\\\n", "<br>");
                        match table.rows.last_mut() {
                            Some(row) => row.push(cell),
                            None => table.rows.push(vec![cell]),
                        }
                    }
                    return;
                }
                "tr" if table.nested == 0 => {
                    table.rows.push(Vec::new());
                    return;
                }
                "table" => {
                    let table = self.table.take().unwrap_or_default();
                    self.inline.clear();
                    self.write_table(table.rows);
                    return;
                }
                _ if is_block(name) => return,
                _ => {}
            }
        }
        match name {
            "details" if self.document => {
                self.flush();
                self.emit("</details>");
                self.rich = true;
            }
            "summary" if self.document => {
                self.flush();
                self.in_summary = false;
            }
            "figcaption" if self.document => {
                self.flush();
                self.in_figcaption = false;
            }
            "dt" if self.document => {
                self.flush();
                self.in_dt = false;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "div" | "section" | "article"
            | "header" | "footer" | "main" | "figure" | "figcaption" | "dl" | "dt" | "dd"
            | "address" | "center" | "nav" | "aside" => {
                if self.document && matches!(name, "dd" | "aside") {
                    self.flush();
                    self.end_item();
                    if let Some(i) = self.stack.iter().rposition(|f| *f == Frame::Quote) {
                        self.stack.truncate(i);
                        self.trim_last_prefix();
                    }
                    self.rich = true;
                    self.after_item = false;
                } else {
                    self.flush();
                }
            }
            "blockquote" => {
                self.flush();
                self.end_item();
                if let Some(i) = self.stack.iter().rposition(|f| *f == Frame::Quote) {
                    self.stack.truncate(i);
                    self.trim_last_prefix();
                }
                self.rich = true;
                self.after_item = false;
            }
            "ul" | "ol" => {
                self.flush();
                self.end_item();
                if let Some(i) = (self.stack.iter()).rposition(|f| matches!(f, Frame::List { .. }))
                {
                    self.stack.truncate(i);
                }
                self.rich = true;
                self.after_item = self.stack.iter().any(|f| matches!(f, Frame::Item { .. }));
            }
            "li" => {
                self.flush();
                self.end_item();
            }
            _ => {
                if let Some(i) = self
                    .spans
                    .iter()
                    .rposition(|(span, _)| span_name(span) == name)
                {
                    self.close_spans(i);
                }
            }
        }
    }

    fn text(&mut self, text: &str) {
        if self.skip > 0 {
            return;
        }
        if self.in_head > 0 {
            if self.in_title {
                self.title.push_str(text);
            }
            return;
        }
        if let Some((pre, _)) = self.pre.as_mut() {
            pre.push_str(text);
            return;
        }
        let code = self.spans.iter().any(|(s, _)| *s == Span::Code);
        let mut collapsed = String::with_capacity(text.len());
        let mut space = self.inline.is_empty() || self.inline.ends_with([' ', '\n']);
        for c in text.chars() {
            if c.is_whitespace() {
                if !space {
                    collapsed.push(' ');
                    space = true;
                }
            } else {
                collapsed.push(c);
                space = false;
            }
        }
        if code {
            self.inline.push_str(&collapsed);
        } else {
            self.inline.push_str(&escape(&collapsed));
        }
    }

    /// 关闭第 `from` 层及更内层的行内样式。
    fn close_spans(&mut self, from: usize) {
        while self.spans.len() > from {
            let Some((span, at)) = self.spans.pop() else {
                break;
            };
            let at = at.min(self.inline.len());
            let inner = self.inline.split_off(at);
            match span {
                Span::Plain(_) => self.inline.push_str(&inner),
                Span::Mark(_, mark) => {
                    let inner = inner.strip_prefix(mark).unwrap_or(&inner);
                    self.wrap(inner, mark, mark);
                }
                Span::Html(tag) => {
                    let open = format!("<{tag}>");
                    let close = format!("</{tag}>");
                    self.wrap(&inner, &open, &close);
                }
                Span::Code => {
                    let ticks = "`".repeat(longest_run(&inner, '`') + 1);
                    let pad = if inner.starts_with('`') || inner.ends_with('`') {
                        " "
                    } else {
                        ""
                    };
                    let inner = format!("{pad}{}{pad}", inner.trim());
                    if inner.trim().is_empty() {
                        continue;
                    }
                    self.wrap(&inner, &ticks, &ticks);
                }
                Span::Link(href) => {
                    let resolved = self.resolve_url(&href);
                    if let Some(href) = resolved
                        && !inner.trim().is_empty()
                    {
                        let close = format!("]({})", destination(&href));
                        self.wrap(&inner, "[", &close);
                    } else {
                        self.inline.push_str(&inner);
                    }
                }
            }
        }
    }

    /// 用 `open`、`close` 包住 `inner`，两端空白移到标记外；只有空白时不加标记。
    fn wrap(&mut self, inner: &str, open: &str, close: &str) {
        let body = inner.trim();
        if body.is_empty() {
            self.inline.push_str(inner);
            return;
        }
        let lead = &inner[..inner.len() - inner.trim_start().len()];
        let trail = &inner[inner.trim_end().len()..];
        self.inline
            .push_str(&format!("{lead}{open}{body}{close}{trail}"));
        self.rich = true;
    }

    /// 写出行内缓冲中的文字。
    fn flush(&mut self) {
        self.close_spans(0);
        let text = std::mem::take(&mut self.inline);
        let text = trim_breaks(&text);
        let heading = self.heading.take();
        if text.is_empty() {
            return;
        }
        match heading {
            Some(level) => {
                if level == 1 {
                    self.has_h1 = true;
                }
                let text = text.replace("\\\n", " ");
                self.emit(&format!("{} {text}", "#".repeat(level)));
                self.rich = true;
            }
            None if self.in_summary => {
                let text = text.replace("\\\n", " ");
                self.emit(&format!("<summary>{text}</summary>"));
                self.rich = true;
            }
            None if self.in_dt => {
                let text = escape_start(text);
                self.emit(&format!("**{text}**"));
                self.rich = true;
            }
            None if self.in_figcaption => {
                let text = escape_start(text);
                self.emit(&format!("*{text}*"));
                self.rich = true;
            }
            None => {
                let text = text.to_owned();
                self.emit(&escape_start(&text));
            }
        }
    }

    /// 关闭引用块后把 `last_prefix` 缩减到当前栈对应的前缀，避免相邻两个引用块粘连。
    fn trim_last_prefix(&mut self) {
        let mut keep = 0;
        for frame in &self.stack {
            keep += match frame {
                Frame::Quote => 2,
                Frame::Item { indent } => *indent,
                Frame::List { .. } => 0,
            };
        }
        if self.last_prefix.len() > keep {
            self.last_prefix.truncate(keep);
        }
    }

    /// 结束当前列表项；没有内容的项只写出记号。
    fn end_item(&mut self) {
        if let Some(i) = self
            .stack
            .iter()
            .rposition(|f| matches!(f, Frame::Item { .. }))
            && !self.stack[i + 1..]
                .iter()
                .any(|f| matches!(f, Frame::List { .. }))
        {
            if self.marker.is_some() {
                self.emit("");
            }
            self.stack.truncate(i);
        }
    }

    /// 写出一块 `text`，每行加上所在引用、列表项的前缀。
    fn emit(&mut self, text: &str) {
        let marker = self.marker.take();
        let (mut first, mut rest) = (String::new(), String::new());
        let innermost = self
            .stack
            .iter()
            .rposition(|f| matches!(f, Frame::Item { .. }));
        for (i, frame) in self.stack.iter().enumerate() {
            match frame {
                Frame::Quote => {
                    first.push_str("> ");
                    rest.push_str("> ");
                }
                Frame::Item { indent } => {
                    let pad = " ".repeat(*indent);
                    match (&marker, Some(i) == innermost) {
                        (Some(marker), true) => first.push_str(marker),
                        _ => first.push_str(&pad),
                    }
                    rest.push_str(&pad);
                }
                Frame::List { .. } => {}
            }
        }
        if !self.out.is_empty() {
            self.out.push('\n');
            if !(marker.is_some() && self.after_item) {
                let shared = (self.last_prefix.chars().zip(rest.chars()))
                    .take_while(|(a, b)| a == b)
                    .count();
                let blank: String = rest.chars().take(shared).collect();
                self.out.push_str(blank.trim_end());
                self.out.push('\n');
            }
        }
        for (k, line) in text.split('\n').enumerate() {
            if k > 0 {
                self.out.push('\n');
            }
            let prefix = if k == 0 { &first } else { &rest };
            self.out.push_str(format!("{prefix}{line}").trim_end());
        }
        if marker.is_some() {
            self.rich = true;
        }
        self.after_item = innermost.is_some();
        self.last_prefix = rest;
    }

    fn write_table(&mut self, rows: Vec<Vec<String>>) {
        let rows: Vec<Vec<String>> = rows.into_iter().filter(|r| !r.is_empty()).collect();
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if rows.is_empty() || columns == 0 {
            return;
        }
        // 只有一格的表格作为一段文字。
        if rows.len() == 1 && columns == 1 {
            self.emit(&rows[0][0]);
            return;
        }
        let line = |row: &[String]| {
            let cells: Vec<&str> = (0..columns)
                .map(|c| row.get(c).map_or("", String::as_str))
                .collect();
            format!("| {} |", cells.join(" | "))
        };
        let mut text = line(&rows[0]);
        text.push('\n');
        text.push_str(&line(&vec!["---".to_owned(); columns]));
        for row in &rows[1..] {
            text.push('\n');
            text.push_str(&line(row));
        }
        self.emit(&text);
        self.rich = true;
    }

    fn finish(mut self) -> Converted {
        if self.pre.is_some() {
            self.close("pre");
        }
        if let Some(table) = self.table.take() {
            self.write_table(table.rows);
        }
        self.flush();
        let mut text = self.out;
        if self.document && !self.has_h1 {
            let title = collapse_whitespace(&self.title);
            if !title.is_empty() {
                let heading = format!("# {}", escape(&title));
                text = if text.is_empty() {
                    heading
                } else {
                    format!("{heading}\n\n{text}")
                };
                self.rich = true;
            }
        }
        text.push('\n');
        Converted {
            text,
            rich: self.rich,
        }
    }
}

/// 是否为块元素（段落、标题、列表、引用等）。
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "h1" | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "p"
            | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "figure"
            | "figcaption"
            | "dl"
            | "dt"
            | "dd"
            | "details"
            | "summary"
            | "address"
            | "center"
            | "nav"
            | "aside"
            | "blockquote"
            | "ul"
            | "ol"
            | "li"
            | "pre"
            | "hr"
    )
}

/// 地址 `url` 是否为绝对地址（带协议或 `mailto:`）；相对地址不写成链接和图片。
fn absolute(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.contains("://") || url.starts_with("mailto:")
}

/// 文档模式下解析链接/图片地址：过滤危险或内联脚本协议，保留相对路径与 `#锚点`，并在有 `<base href>` 时补全。
fn resolve_doc_url(url: &str, base: Option<&str>) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    if ["javascript:", "vbscript:", "data:", "blob:", "about:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return None;
    }
    if url.starts_with('#') || url.contains("://") || lower.starts_with("mailto:") {
        return Some(url.to_owned());
    }
    if url.starts_with("//") {
        let scheme = if base.is_some_and(|b| b.trim_start().to_ascii_lowercase().starts_with("http://")) {
            "http:"
        } else {
            "https:"
        };
        return Some(format!("{scheme}{url}"));
    }
    if let Some(base) = base.map(str::trim).filter(|b| {
        let low = b.to_ascii_lowercase();
        low.starts_with("http://") || low.starts_with("https://")
    }) {
        return Some(join_http_url(base, url));
    }
    Some(url.to_owned())
}

fn join_http_url(base: &str, rel: &str) -> String {
    let Some((scheme, rest)) = base.split_once("://") else {
        return rel.to_owned();
    };
    let (authority, base_path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let base_path = base_path.split(['?', '#']).next().unwrap_or("/");
    if rel.starts_with('/') {
        return format!("{scheme}://{authority}{rel}");
    }
    if rel.starts_with('?') {
        return format!("{scheme}://{authority}{base_path}{rel}");
    }
    let dir = match base_path.rfind('/') {
        Some(i) => &base_path[..=i],
        None => "/",
    };
    let split_at = rel.find(['?', '#']).unwrap_or(rel.len());
    let (rel_path, suffix) = rel.split_at(split_at);
    let mut segments: Vec<&str> = Vec::new();
    for seg in dir.split('/').chain(rel_path.split('/')) {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    let joined = segments.join("/");
    let trailing = if rel_path.ends_with('/') && !joined.is_empty() {
        "/"
    } else {
        ""
    };
    format!("{scheme}://{authority}/{joined}{trailing}{suffix}")
}

fn has_flag(attrs: &str, key: &str) -> bool {
    attrs.split_whitespace().any(|w| w.eq_ignore_ascii_case(key))
}

fn first_srcset(srcset: &str) -> Option<String> {
    srcset
        .split(',')
        .next()?
        .split_whitespace()
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
                space = true;
            }
        } else {
            out.push(c);
            space = false;
        }
    }
    out.trim_end().to_owned()
}

fn span_name(span: &Span) -> &str {
    match span {
        Span::Mark(name, _) | Span::Plain(name) => name,
        Span::Html(tag) => tag,
        Span::Code => "code",
        Span::Link(_) => "a",
    }
}

/// 返回 `class` 中 `language-`、`lang-` 之后的语言名，或 `data-lang`/`data-language` 属性。
fn code_lang(attrs: &str) -> String {
    let class = attr(attrs, "class").unwrap_or_default();
    if let Some(lang) = (class.split_whitespace()).find_map(|c| {
        c.strip_prefix("language-")
            .or_else(|| c.strip_prefix("lang-"))
    }) {
        return lang.to_owned();
    }
    attr(attrs, "data-lang")
        .or_else(|| attr(attrs, "data-language"))
        .unwrap_or_default()
}

fn longest_run(text: &str, c: char) -> usize {
    let (mut best, mut run) = (0, 0);
    for ch in text.chars() {
        run = if ch == c { run + 1 } else { 0 };
        best = best.max(run);
    }
    best
}

/// 转义会被解析为标记的字符：反斜杠、`*`、反引号、方括号、`<`、`~`、`$`，以及不在词中的 `_`。
fn escape(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let inside_word = |k: Option<&char>| k.is_some_and(|c| c.is_alphanumeric());
        let underscore = c == '_'
            && !(inside_word(i.checked_sub(1).and_then(|k| chars.get(k)))
                && inside_word(chars.get(i + 1)));
        if matches!(c, '\\' | '*' | '`' | '[' | ']' | '<' | '~' | '$') || underscore {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 去掉 `text` 两端的空白和换行（`\` 加换行）。
fn trim_breaks(text: &str) -> &str {
    let mut text = text.trim();
    loop {
        let trimmed = (text.strip_prefix("\\\n").map(str::trim_start)).or_else(|| {
            text.strip_suffix('\\')
                .filter(|t| !escaped(t))
                .map(str::trim_end)
        });
        match trimmed {
            Some(t) => text = t,
            None => return text,
        }
    }
}

/// `text` 末尾是否有奇数个反斜杠，即紧随其后的字符被转义。
fn escaped(text: &str) -> bool {
    text.bytes().rev().take_while(|&b| b == b'\\').count() % 2 == 1
}

/// 段首像标题、引用或列表记号时在前面加反斜杠。
fn escape_start(text: &str) -> String {
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    let listy = (digits > 0 && text[digits..].starts_with(". "))
        || ["- ", "+ ", "> ", "#"].iter().any(|p| text.starts_with(p));
    match listy {
        true => format!("\\{text}"),
        false => text.to_owned(),
    }
}

#[cfg(test)]
mod tests;
