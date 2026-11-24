//! 读 pptx 演示文稿（Office Open XML 的 PresentationML），按幻灯片顺序转换成 Markdown 文本。
//!
//! 转换结果交给 Markdown 预览排版，得到便于阅读的视图，不还原版式与动画：每张幻灯片的标题、
//! 段落、项目符号与编号列表、表格和嵌入图片依次输出，幻灯片之间以分隔线隔开。
//!
//! 元素与属性按本地名称识别，不区分命名空间前缀，ECMA-376 的过渡与严格两种命名空间都适用。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use roxmltree::Node;
use winc_i18n::t;

use crate::convert::{escape, html_attr, link_target, previewable_image};
use crate::ooxml::{package_target, parse, part, relations};
use crate::zip::{self, Archive};

/// 包关系中没有主演示文稿时的默认位置。
const DEFAULT_PRESENTATION: &str = "ppt/presentation.xml";

/// 能读的 pptx 文件的大小上限（字节）：与 xlsx、docx 预览使用同一上限（256 MiB）。
pub const MAX_FILE: u64 = crate::table::MAX_FILE;

/// 幻灯片列表嵌套的最大层级（0 起）。
const MAX_LIST_LEVEL: usize = 8;

/// 转换结果：Markdown 文本，以及其中引用的、包内的图片。
#[derive(Debug, Default)]
pub struct Converted {
    /// 转换得到的 Markdown 文本。
    pub markdown: String,
    /// 图片：Markdown 中引用的地址（包中的路径）和图片文件的内容。
    pub media: Vec<(String, Arc<[u8]>)>,
}

/// `path` 是否是 pptx 演示文稿（按扩展名判断，含放映文件 ppsx 和模板 potx）。
pub fn is_pptx(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["pptx", "ppsx", "potx"]
            .iter()
            .any(|known| e.eq_ignore_ascii_case(known))
    })
}

/// 读取 pptx 文件 `path` 并转换成 Markdown。
///
/// # Errors
///
/// 文件无法读取、超过大小上限（256 MiB），或内容不是有效的 pptx 时返回给用户看的原因。
pub fn load(path: &Path) -> Result<Converted, String> {
    let metadata = winc_backend::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len > MAX_FILE {
        return Err(t!("preview-pptx-too-large").to_owned());
    }
    let bytes = winc_backend::fs::read(path).map_err(|e| e.to_string())?;
    convert(&bytes)
}

/// 把内存中的 pptx 文件 `bytes` 转换成 Markdown。
///
/// # Errors
///
/// 不是 zip 压缩包、找不到幻灯片，或其中的 XML 损坏时返回给用户看的原因。
pub fn convert(bytes: &[u8]) -> Result<Converted, String> {
    let archive = Archive::open(bytes).map_err(|e| match e {
        zip::Error::Invalid => t!("preview-pptx-invalid").to_owned(),
        zip::Error::Unsupported => t!("preview-pptx-unsupported").to_owned(),
        zip::Error::TooLarge => t!("preview-pptx-too-large").to_owned(),
    })?;
    let converted = convert_archive(&archive);
    if archive.exceeded() {
        return Err(t!("preview-pptx-too-large").to_owned());
    }
    converted
}

/// 把打开的 pptx 包 `archive` 转换成 Markdown，见 [`convert`]。
fn convert_archive(archive: &Archive<'_>) -> Result<Converted, String> {
    let slides = slide_paths(archive)?;
    if slides.is_empty() {
        return Err(t!("preview-pptx-invalid").to_owned());
    }

    let mut sections: Vec<String> = Vec::with_capacity(slides.len());
    let mut media: Vec<(String, Arc<[u8]>)> = Vec::new();

    for (idx, slide_path) in slides.iter().enumerate() {
        let Some(bytes) = part(archive, slide_path)? else {
            continue;
        };
        let doc = parse(&bytes, slide_path)?;
        let rels = relations(archive, slide_path).unwrap_or_default();
        let mut cx = SlideConverter {
            archive,
            rels: &rels,
            media: &mut media,
            blocks: Vec::new(),
        };
        let section = cx.convert_slide(doc.root_element(), idx + 1);
        if !section.is_empty() {
            sections.push(section);
        }
    }

    let mut markdown = sections.join("\n\n---\n\n");
    if !markdown.is_empty() {
        markdown.push('\n');
    }
    Ok(Converted { markdown, media })
}

/// 按演示文稿的顺序返回各张幻灯片在包中的路径。
///
/// 优先从 `ppt/presentation.xml` 的 `sldIdLst` 与关系文件读取顺序；缺失时回退为按编号排序的
/// `ppt/slides/slide*.xml`。
fn slide_paths(archive: &Archive<'_>) -> Result<Vec<String>, String> {
    let main = package_target(archive).unwrap_or_else(|| DEFAULT_PRESENTATION.to_owned());
    if let Some(bytes) = part(archive, &main)? {
        let doc = parse(&bytes, &main)?;
        let rels = relations(archive, &main).unwrap_or_default();
        if let Some(list) = doc
            .root_element()
            .descendants()
            .find(|n| n.tag_name().name() == "sldIdLst")
        {
            let ordered: Vec<String> = list
                .children()
                .filter(|n| n.tag_name().name() == "sldId")
                .filter_map(|n| rel_attr(n, "id"))
                .filter_map(|rid| rels.get(rid).map(|(_, target)| target.clone()))
                .collect();
            if !ordered.is_empty() {
                return Ok(ordered);
            }
        }
    }

    let mut fallback: Vec<(u32, String)> = archive
        .names()
        .filter_map(|name| {
            let lower = name.to_ascii_lowercase();
            let rest = lower.strip_prefix("ppt/slides/slide")?;
            let digits = rest.strip_suffix(".xml")?;
            let num = digits.parse::<u32>().ok()?;
            Some((num, name.to_owned()))
        })
        .collect();
    fallback.sort_by_key(|(num, _)| *num);
    Ok(fallback.into_iter().map(|(_, path)| path).collect())
}

/// 返回 `node` 本地名称为 `name` 的属性，不区分命名空间。
fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

/// 返回 `node` 中引用关系编号的属性（`r:id`、`r:embed`）：命名空间为关系命名空间。
fn rel_attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| {
            a.name() == name
                && a.namespace()
                    .is_some_and(|ns| ns.ends_with("/relationships"))
        })
        .map(|a| a.value())
}

/// 返回 `node` 中本地名称为 `name` 的第一个子元素。
fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| n.tag_name().name() == name)
}

/// 布尔属性（`b="1"`、`i="true"`）是否为真。
fn attr_true(node: Node<'_, '_>, name: &str) -> bool {
    matches!(attr(node, name), Some("1" | "true"))
}

/// 占位符的种类（`<p:ph type="..."/>`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    Title,
    Subtitle,
    Body,
    Meta,
    Other,
}

/// 识别形状 `sp` 的占位符种类。
fn placeholder_kind(sp: Node<'_, '_>) -> Placeholder {
    let ph = child(sp, "nvSpPr")
        .and_then(|nv| child(nv, "nvPr"))
        .and_then(|nv| child(nv, "ph"));
    let Some(ph) = ph else {
        return Placeholder::Other;
    };
    match attr(ph, "type") {
        Some("title" | "ctrTitle") => Placeholder::Title,
        Some("subTitle") => Placeholder::Subtitle,
        Some("body" | "obj") => Placeholder::Body,
        Some("sldNum" | "ftr" | "dt" | "hdr") => Placeholder::Meta,
        _ => Placeholder::Other,
    }
}

/// 行内文字的样式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Format {
    bold: bool,
    italic: bool,
    strike: bool,
}

impl Format {
    fn from_rpr(rpr: Node<'_, '_>) -> Self {
        let strike = attr(rpr, "strike").is_some_and(|v| v != "noStrike");
        Self {
            bold: attr_true(rpr, "b"),
            italic: attr_true(rpr, "i"),
            strike,
        }
    }

    fn open(self, out: &mut String) {
        if self.bold {
            out.push_str("<b>");
        }
        if self.italic {
            out.push_str("<i>");
        }
        if self.strike {
            out.push_str("<s>");
        }
    }

    fn close(self, out: &mut String) {
        if self.strike {
            out.push_str("</s>");
        }
        if self.italic {
            out.push_str("</i>");
        }
        if self.bold {
            out.push_str("</b>");
        }
    }
}

/// 单张幻灯片的转换状态。
struct SlideConverter<'a> {
    archive: &'a Archive<'a>,
    rels: &'a HashMap<String, (String, String)>,
    media: &'a mut Vec<(String, Arc<[u8]>)>,
    /// 转换出的块：文本与是否为列表项（相邻列表项之间不空行）。
    blocks: Vec<(String, bool)>,
}

impl SlideConverter<'_> {
    /// 把一张幻灯片的根元素 `root` 转换成 Markdown 段落，序号从 1 起。
    fn convert_slide(&mut self, root: Node<'_, '_>, number: usize) -> String {
        let Some(sp_tree) = child(root, "cSld").and_then(|c| child(c, "spTree")) else {
            return format!("## {}", t!("preview-pptx-slide", number = number));
        };

        let title = self.find_title(sp_tree);
        let heading = match title {
            Some(ref title_text) => {
                format!("## {} — {title_text}", t!("preview-pptx-slide", number = number))
            }
            None => format!("## {}", t!("preview-pptx-slide", number = number)),
        };
        self.blocks.push((heading, false));

        self.walk_tree(sp_tree, title.is_some());

        let mut out = String::new();
        let mut prev_list = false;
        for (text, is_list) in self.blocks.iter().filter(|(t, _)| !t.is_empty()) {
            if !out.is_empty() {
                out.push_str(if *is_list && prev_list { "\n" } else { "\n\n" });
            }
            out.push_str(text);
            prev_list = *is_list;
        }
        out
    }

    /// 查找幻灯片中的主标题文字（若存在非空的 `title` / `ctrTitle` 占位符）。
    fn find_title(&self, tree: Node<'_, '_>) -> Option<String> {
        for node in tree.children().filter(Node::is_element) {
            if node.tag_name().name() == "sp" && placeholder_kind(node) == Placeholder::Title {
                let text = self.plain_shape_text(node);
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        None
    }

    /// 提取形状内所有段落的纯文字（用于幻灯片标题行）。
    fn plain_shape_text(&self, sp: Node<'_, '_>) -> String {
        let Some(tx_body) = child(sp, "txBody") else {
            return String::new();
        };
        let parts: Vec<String> = tx_body
            .children()
            .filter(|n| n.tag_name().name() == "p")
            .map(|p| self.inline(p).trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        parts.join(" ")
    }

    /// 遍历形状树（含分组形状 `grpSp`），提取文本、表格和图片。
    fn walk_tree(&mut self, parent: Node<'_, '_>, skip_first_title: bool) {
        let mut skipped_title = false;
        for node in parent.children().filter(Node::is_element) {
            match node.tag_name().name() {
                "sp" => {
                    let kind = placeholder_kind(node);
                    if kind == Placeholder::Meta {
                        continue;
                    }
                    if kind == Placeholder::Title && skip_first_title && !skipped_title {
                        skipped_title = true;
                        continue;
                    }
                    self.shape(node, kind);
                }
                "grpSp" => self.walk_tree(node, false),
                "graphicFrame" => self.graphic_frame(node),
                "pic" => self.picture(node),
                _ => {}
            }
        }
    }

    /// 转换文本形状 `<p:sp>` 中的各段落。
    fn shape(&mut self, sp: Node<'_, '_>, kind: Placeholder) {
        let Some(tx_body) = child(sp, "txBody") else {
            return;
        };
        let paragraphs: Vec<Node<'_, '_>> = tx_body
            .children()
            .filter(|n| n.tag_name().name() == "p")
            .collect();
        let multi = paragraphs.len() > 1;
        for p in paragraphs {
            let content = self.inline(p);
            let text = content.trim();
            if text.is_empty() {
                continue;
            }
            if kind == Placeholder::Subtitle {
                self.blocks.push((format!("### {text}"), false));
                continue;
            }
            let ppr = child(p, "pPr");
            let lvl = ppr
                .and_then(|n| attr(n, "lvl")?.parse::<usize>().ok())
                .unwrap_or(0)
                .min(MAX_LIST_LEVEL);
            let bu_none = ppr.is_some_and(|n| child(n, "buNone").is_some());
            let bu_auto = ppr.is_some_and(|n| child(n, "buAutoNum").is_some());
            let bu_char = ppr.is_some_and(|n| child(n, "buChar").is_some());

            if !bu_none && bu_auto {
                let indent = "    ".repeat(lvl);
                self.blocks.push((format!("{indent}1. {text}"), true));
            } else if !bu_none && (bu_char || lvl > 0 || (kind == Placeholder::Body && multi)) {
                let indent = "    ".repeat(lvl);
                self.blocks.push((format!("{indent}- {text}"), true));
            } else {
                self.blocks.push((text.to_owned(), false));
            }
        }
    }

    /// 转换段落 `<a:p>` 内的行内元素（文字、样式、链接、换行）。
    fn inline(&self, p: Node<'_, '_>) -> String {
        let mut runs: Vec<(String, Format)> = Vec::new();
        let mut out = String::new();
        let flush = |runs: &mut Vec<(String, Format)>, out: &mut String| {
            let mut i = 0;
            while i < runs.len() {
                let style = runs[i].1;
                let mut text = String::new();
                while i < runs.len() && runs[i].1 == style {
                    text.push_str(&runs[i].0);
                    i += 1;
                }
                style.open(out);
                out.push_str(&text);
                style.close(out);
            }
            runs.clear();
        };

        for node in p.children().filter(Node::is_element) {
            match node.tag_name().name() {
                "r" | "fld" => {
                    let rpr = child(node, "rPr");
                    let style = rpr.map(Format::from_rpr).unwrap_or_default();
                    let link = rpr
                        .and_then(|r| child(r, "hlinkClick"))
                        .and_then(|h| rel_attr(h, "id"))
                        .and_then(|id| self.rels.get(id))
                        .map(|(_, target)| target.as_str())
                        .filter(|t| !t.is_empty());
                    let raw: String = node
                        .children()
                        .filter(|n| n.tag_name().name() == "t")
                        .filter_map(|n| n.text())
                        .collect();
                    if raw.is_empty() {
                        continue;
                    }
                    let mut escaped = String::new();
                    escape(&raw, &mut escaped);
                    if let Some(url) = link {
                        flush(&mut runs, &mut out);
                        let mut inner = String::new();
                        style.open(&mut inner);
                        inner.push_str(&escaped);
                        style.close(&mut inner);
                        out.push_str(&format!("[{inner}](<{}>)", link_target(url)));
                    } else {
                        runs.push((escaped, style));
                    }
                }
                "br" => {
                    runs.push(("<br>".to_owned(), Format::default()));
                }
                _ => {}
            }
        }
        flush(&mut runs, &mut out);
        out
    }

    /// 转换 `<p:graphicFrame>` 中的表格 `<a:tbl>`。
    fn graphic_frame(&mut self, frame: Node<'_, '_>) {
        if let Some(tbl) = frame.descendants().find(|n| n.tag_name().name() == "tbl") {
            let table = self.table(tbl);
            if !table.is_empty() {
                self.blocks.push((table, false));
            }
        }
    }

    /// 转换 `DrawingML` 表格 `<a:tbl>` 为 GFM 表格。
    fn table(&self, tbl: Node<'_, '_>) -> String {
        let mut rows: Vec<Vec<String>> = Vec::new();
        for tr in tbl.children().filter(|n| n.tag_name().name() == "tr") {
            let mut row = Vec::new();
            for tc in tr.children().filter(|n| n.tag_name().name() == "tc") {
                if attr_true(tc, "hMerge") {
                    continue;
                }
                let span = crate::convert::span(attr(tc, "gridSpan"));
                let text = if attr_true(tc, "vMerge") {
                    String::new()
                } else {
                    self.cell(tc)
                };
                row.push(text);
                row.extend(std::iter::repeat_n(String::new(), span - 1));
            }
            if !row.is_empty() {
                rows.push(row);
            }
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return String::new();
        }
        let mut out = String::new();
        for (k, row) in rows.iter().enumerate() {
            out.push('|');
            for c in 0..columns {
                out.push(' ');
                out.push_str(row.get(c).map_or("", String::as_str));
                out.push_str(" |");
            }
            out.push('\n');
            if k == 0 {
                out.push('|');
                out.push_str(&" --- |".repeat(columns));
                out.push('\n');
            }
        }
        out.pop();
        out
    }

    /// 转换表格单元格 `<a:tc>`：各段落以 `<br>` 连接。
    fn cell(&self, tc: Node<'_, '_>) -> String {
        let Some(tx_body) = child(tc, "txBody") else {
            return String::new();
        };
        let lines: Vec<String> = tx_body
            .children()
            .filter(|n| n.tag_name().name() == "p")
            .map(|p| self.inline(p).trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        lines.join("<br>")
    }

    /// 转换图片 `<p:pic>`：从关系中取图片路径、读入 `media` 并输出 `<img>`。
    fn picture(&mut self, pic: Node<'_, '_>) {
        let rid = pic
            .descendants()
            .filter(|n| n.tag_name().name() == "blip")
            .find_map(|n| rel_attr(n, "embed").or_else(|| rel_attr(n, "link")));
        let Some(path) = rid
            .and_then(|id| self.rels.get(id))
            .map(|(_, target)| target.clone())
        else {
            return;
        };
        let name = path.rsplit('/').next().unwrap_or(&path);
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !previewable_image(&ext) {
            return;
        }
        if !self.media.iter().any(|(url, _)| *url == path)
            && let Some(Ok(bytes)) = self.archive.read(&path)
        {
            self.media.push((path.clone(), bytes.into()));
        }
        let alt = pic
            .descendants()
            .find(|n| n.tag_name().name() == "cNvPr")
            .and_then(|n| {
                attr(n, "descr")
                    .filter(|d| !d.is_empty())
                    .or_else(|| attr(n, "name").filter(|s| !s.is_empty()))
            })
            .unwrap_or_default();
        let html = format!(
            "<img src=\"{}\" alt=\"{}\">",
            html_attr(&link_target(&path)),
            html_attr(alt)
        );
        self.blocks.push((html, false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P_NS: &str = "xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" \
        xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
        xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";
    const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    fn build_pptx(extra: &[(&str, &[u8])]) -> Vec<u8> {
        let root_rels = format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"{REL_NS}/officeDocument\" Target=\"ppt/presentation.xml\"/>\
             </Relationships>"
        );
        let mut files: Vec<(&str, &[u8])> = vec![("_rels/.rels", root_rels.as_bytes())];
        files.extend_from_slice(extra);
        crate::zip::build(&files, true)
    }

    #[test]
    fn is_pptx_matches_expected_extensions() {
        assert!(is_pptx(Path::new("deck.pptx")));
        assert!(is_pptx(Path::new("DECK.PPTX")));
        assert!(is_pptx(Path::new("show.ppsx")));
        assert!(is_pptx(Path::new("template.potx")));
        assert!(!is_pptx(Path::new("doc.docx")));
        assert!(!is_pptx(Path::new("notes.md")));
    }

    #[test]
    fn extracts_ordered_slides_text_tables_and_images() {
        // presentation.xml lists slide2 BEFORE slide1 to verify slide ordering follows sldIdLst.
        let pres = format!(
            "<p:presentation {P_NS}>\
             <p:sldIdLst>\
               <p:sldId id=\"256\" r:id=\"rId2\"/>\
               <p:sldId id=\"257\" r:id=\"rId1\"/>\
             </p:sldIdLst>\
             </p:presentation>"
        );
        let pres_rels = format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"{REL_NS}/slide\" Target=\"slides/slide1.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"{REL_NS}/slide\" Target=\"slides/slide2.xml\"/>\
             </Relationships>"
        );
        let slide2 = format!(
            "<p:sld {P_NS}><p:cSld><p:spTree>\
             <p:sp>\
               <p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr/><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr>\
               <p:txBody><a:bodyPr/><a:p><a:r><a:t>Overview</a:t></a:r></a:p></p:txBody>\
             </p:sp>\
             <p:sp>\
               <p:nvSpPr><p:cNvPr id=\"3\" name=\"Body\"/><p:cNvSpPr/><p:nvPr><p:ph type=\"body\"/></p:nvPr></p:nvSpPr>\
               <p:txBody><a:bodyPr/>\
                 <a:p><a:pPr lvl=\"0\"><a:buChar char=\"•\"/></a:pPr><a:r><a:rPr b=\"1\"/><a:t>Fast</a:t></a:r></a:p>\
                 <a:p><a:pPr lvl=\"1\"><a:buChar char=\"•\"/></a:pPr><a:r><a:t>Low memory</a:t></a:r></a:p>\
               </p:txBody>\
             </p:sp>\
             </p:spTree></p:cSld></p:sld>"
        );
        let slide1 = format!(
            "<p:sld {P_NS}><p:cSld><p:spTree>\
             <p:sp>\
               <p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr/><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr>\
               <p:txBody><a:bodyPr/><a:p><a:r><a:t>Metrics</a:t></a:r></a:p></p:txBody>\
             </p:sp>\
             <p:graphicFrame><a:graphic><a:graphicData><a:tbl>\
               <a:tr>\
                 <a:tc><a:txBody><a:p><a:r><a:t>Name</a:t></a:r></a:p></a:txBody></a:tc>\
                 <a:tc><a:txBody><a:p><a:r><a:t>Value</a:t></a:r></a:p></a:txBody></a:tc>\
               </a:tr>\
               <a:tr>\
                 <a:tc><a:txBody><a:p><a:r><a:t>Latency</a:t></a:r></a:p></a:txBody></a:tc>\
                 <a:tc><a:txBody><a:p><a:r><a:t>12ms</a:t></a:r></a:p></a:txBody></a:tc>\
               </a:tr>\
             </a:tbl></a:graphicData></a:graphic></p:graphicFrame>\
             <p:pic>\
               <p:nvPicPr><p:cNvPr id=\"5\" name=\"Chart\" descr=\"System diagram\"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>\
               <p:blipFill><a:blip r:embed=\"rImg1\"/></p:blipFill>\
             </p:pic>\
             </p:spTree></p:cSld></p:sld>"
        );
        let slide1_rels = format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rImg1\" Type=\"{REL_NS}/image\" Target=\"../media/image1.png\"/>\
             </Relationships>"
        );
        let fake_png: &[u8] = b"\x89PNG\r\n\x1a\n";

        let bytes = build_pptx(&[
            ("ppt/presentation.xml", pres.as_bytes()),
            ("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
            ("ppt/slides/slide1.xml", slide1.as_bytes()),
            ("ppt/slides/_rels/slide1.xml.rels", slide1_rels.as_bytes()),
            ("ppt/slides/slide2.xml", slide2.as_bytes()),
            ("ppt/media/image1.png", fake_png),
        ]);

        let converted = convert(&bytes).expect("valid pptx should convert");
        let md = &converted.markdown;

        // Slide order: Overview (from slide2) appears before Metrics (from slide1).
        let pos_overview = md.find("Overview").expect("Overview title missing");
        let pos_metrics = md.find("Metrics").expect("Metrics title missing");
        assert!(pos_overview < pos_metrics, "slides must follow sldIdLst order:\n{md}");

        // Bullets and bold formatting.
        assert!(md.contains("- <b>Fast</b>"), "missing bold bullet:\n{md}");
        assert!(md.contains("    - Low memory"), "missing nested bullet:\n{md}");

        // GFM table.
        assert!(md.contains("| Name | Value |"), "missing table header:\n{md}");
        assert!(md.contains("| --- | --- |"), "missing table separator:\n{md}");
        assert!(md.contains("| Latency | 12ms |"), "missing table row:\n{md}");

        // Embedded image.
        assert!(
            md.contains("<img src=\"ppt/media/image1.png\" alt=\"System diagram\">"),
            "missing image ref:\n{md}"
        );
        assert_eq!(converted.media.len(), 1);
        assert_eq!(converted.media[0].0, "ppt/media/image1.png");
        assert_eq!(converted.media[0].1.as_ref(), fake_png);
    }

    #[test]
    fn falls_back_to_sorted_slide_filenames_when_presentation_missing() {
        let slide1 = format!(
            "<p:sld {P_NS}><p:cSld><p:spTree>\
             <p:sp><p:txBody><a:p><a:r><a:t>First</a:t></a:r></a:p></p:txBody></p:sp>\
             </p:spTree></p:cSld></p:sld>"
        );
        let slide2 = format!(
            "<p:sld {P_NS}><p:cSld><p:spTree>\
             <p:sp><p:txBody><a:p><a:r><a:t>Second</a:t></a:r></a:p></p:txBody></p:sp>\
             </p:spTree></p:cSld></p:sld>"
        );
        let bytes = crate::zip::build(
            &[
                ("ppt/slides/slide10.xml", slide2.as_bytes()),
                ("ppt/slides/slide2.xml", slide1.as_bytes()),
            ],
            true,
        );
        let converted = convert(&bytes).expect("fallback slide discovery should work");
        let p1 = converted.markdown.find("First").unwrap();
        let p2 = converted.markdown.find("Second").unwrap();
        assert!(p1 < p2, "numeric sort should place slide2 before slide10");
    }

    #[test]
    fn rejects_invalid_zip() {
        assert!(convert(b"not a zip").is_err());
    }
}
