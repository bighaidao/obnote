//! 剪贴板 HTML 转换为 Markdown 的测试。

use super::*;

fn md(html: &str) -> String {
    from_html(html).text
}

#[test]
fn blocks_and_inline_styles_follow_the_elements() {
    let html = "<meta charset='utf-8'><!--StartFragment--><h2>安装</h2>\
        <p>先运行 <code>cargo build</code>，再看<a href=\"https://winc.si/docs\">文档</a>。</p>\
        <p><strong>注意</strong>：<em>不要</em> 用 <del>旧版</del>。<br>第二行</p>\
        <blockquote><p>引用一</p><p>引用二</p></blockquote><hr>\
        <img src=\"https://a.dev/x.png\" alt=\"图\"><!--EndFragment-->";
    let out = from_html(html);
    assert!(out.rich);
    assert_eq!(
        out.text,
        "## 安装\n\n先运行 `cargo build`，再看[文档](https://winc.si/docs)。\n\n\
         **注意**：*不要* 用 ~~旧版~~。\\\n第二行\n\n> 引用一\n>\n> 引用二\n\n---\n\n\
         ![图](https://a.dev/x.png)\n"
    );
}

#[test]
fn lists_nest_and_number() {
    let html = "<ul><li>甲<ul><li>甲一</li><li>甲二</li></ul></li><li>乙</li></ul>\
        <ol start=\"3\"><li><p>三</p></li><li>四</ol><p>后文</p>";
    assert_eq!(
        md(html),
        "- 甲\n  - 甲一\n  - 甲二\n- 乙\n\n3. 三\n4. 四\n\n后文\n"
    );
    let tasks =
        "<ul><li><input type=\"checkbox\" checked>做完</li><li><input type=checkbox>没做</li></ul>";
    assert_eq!(md(tasks), "- [x] 做完\n- [ ] 没做\n");
}

#[test]
fn tables_and_code_blocks_keep_their_shape() {
    let html = "<table><thead><tr><th>名称</th><th>说明</th></tr></thead>\
        <tbody><tr><td><p>a|b</p></td><td>一<br>二</td></tr><tr><td>c</td></tr></tbody></table>\
        <pre><code class=\"language-rust\">fn main() {\n    let x = `a`;\n}\n</code></pre>";
    assert_eq!(
        md(html),
        "| 名称 | 说明 |\n| --- | --- |\n| a\\|b | 一<br>二 |\n| c |  |\n\n\
         ```rust\nfn main() {\n    let x = `a`;\n}\n```\n"
    );
}

#[test]
fn online_documents_and_code_editors() {
    // 外层 `<b style="font-weight:normal">` 不表示粗体，`span` 样式中的粗体、斜体生效。
    let docs = "<b style=\"font-weight:normal;\" id=\"docs-internal-guid-1\">\
        <p><span style=\"font-weight:700\">重点</span><span> 和 </span>\
        <span style=\"font-style:italic\">斜体</span></p></b>";
    assert_eq!(md(docs), "**重点** 和 *斜体*\n");
    // 只带颜色的 `span` 没有结构。
    let code = "<div style=\"color: #333;white-space: pre;\"><div><span style=\"color: #00f;\">fn\
        </span> main() {}</div></div>";
    assert!(!from_html(code).rich);
    // 标记字符和段首的列表记号转义，词中的下划线不转义。
    assert_eq!(
        md("<p>1. a*b* snake_case _x_</p>"),
        "\\1. a\\*b\\* snake_case \\_x\\_\n"
    );
    // 脚本、样式、页内链接和 `data:` 图片不输出。
    let noise = "<style>p{}</style><script>x()</script><p><a href=\"#top\">顶部</a>\
        <img src=\"data:image/png;base64,AAAA\" alt=\"嵌入\"></p>";
    assert_eq!(md(noise), "顶部嵌入\n");
}

#[test]
fn skipped_elements_and_relative_addresses() {
    // `head` 中的其他标签和脚本文字中的标签不影响后续内容。
    let html = "<head><base href=\"https://a.dev/\"><title>t</title></head>\
        <script>let s = \"<div>\";</script><h2>标题</h2>";
    assert_eq!(md(html), "## 标题\n");
    // 相对地址的链接和图片只保留文字。
    let html = "<p><a href=\"/guide\">指南</a> <a href=\"mailto:a@b.c\">信</a>\
        <img src=\"logo.png\" alt=\"标志\"></p>";
    assert_eq!(md(html), "指南 [信](mailto:a@b.c)标志\n");
}

#[test]
fn plain_text_stays_plain() {
    // 波浪线和美元符号不成为删除线和公式；段末的换行去掉，文字中的反斜杠保留。
    assert_eq!(
        md("<p>1~2 和 3~4，$5-$10</p>"),
        "1\\~2 和 3\\~4，\\$5-\\$10\n"
    );
    assert_eq!(
        md("<p>路径 C:\\<br></p><p><br>下一段</p>"),
        "路径 C:\\\\\n\n下一段\n"
    );
    // 序号超过 9 位的起始值不用。
    assert_eq!(
        md("<ol start=\"4294967295\"><li>a</li><li>b</li></ol>"),
        "1. a\n2. b\n"
    );
}

fn doc_md(html: &str) -> String {
    from_document(html).text
}

#[test]
fn document_title_and_chrome_filtering() {
    // 无 h1 时保留 <title> 作为一级标题；跳过 nav、svg、noscript、外层 aside 与 footer。
    let html = "<!doctype html><html><head><title>  文档  标题 </title></head><body>\
        <nav><a href=\"/home\">首页</a><a href=\"/about\">关于</a></nav>\
        <aside>侧边栏广告</aside>\
        <main><article>\
            <svg><title>图标</title><path d=\"M0 0\"/></svg>\
            <noscript>需要 JS</noscript>\
            <p>正文段落。</p>\
            <aside><p>提示框</p></aside>\
            <blockquote><p>名言</p><footer>— 作者</footer></blockquote>\
        </article></main>\
        <footer>版权所有</footer>\
        </body></html>";
    assert_eq!(
        doc_md(html),
        "# 文档 标题\n\n正文段落。\n\n> 提示框\n\n> 名言\n>\n> — 作者\n"
    );

    // 已有 h1 时不重复插入 <title>。
    let with_h1 = "<html><head><title>网页标题</title></head>\
        <body><main><h1>真正标题</h1><p>内容</p></main></body></html>";
    assert_eq!(doc_md(with_h1), "# 真正标题\n\n内容\n");
}

#[test]
fn document_relative_urls_and_base_href() {
    // 无 <base> 时保留相对路径、data-src、srcset 与 #锚点，过滤 javascript: 与 data:。
    let local = "<p><a href=\"guide/intro.html\">指南</a> \
        <a href=\"#sec-1\">第一节</a> \
        <a href=\"javascript:void(0)\">脚本</a></p>\
        <p><img src=\"assets/pic.png\" alt=\"图1\"> \
        <img data-src=\"assets/lazy.png\" alt=\"图2\"> \
        <img srcset=\"assets/hi.png 2x, assets/lo.png 1x\" alt=\"图3\"> \
        <img src=\"data:image/png;base64,AAA\" alt=\"内嵌\"></p>";
    assert_eq!(
        doc_md(local),
        "[指南](guide/intro.html) [第一节](#sec-1) 脚本\n\n\
         ![图1](assets/pic.png) ![图2](assets/lazy.png) ![图3](assets/hi.png) 内嵌\n"
    );

    // 有 <base href> 时解析相对路径、根路径、../ 与 // 协议相对地址。
    let remote = "<head><base href=\"https://example.com/docs/v1/page.html\"></head>\
        <body><p><a href=\"../v2/api.html#top\">API</a> \
        <a href=\"/root\">根</a> \
        <img src=\"//cdn.example.com/a.png\" alt=\"cdn\"> \
        <img src=\"img/b.png\" alt=\"b\"></p></body>";
    assert_eq!(
        doc_md(remote),
        "[API](https://example.com/docs/v2/api.html#top) \
         [根](https://example.com/root) \
         ![cdn](https://cdn.example.com/a.png) \
         ![b](https://example.com/docs/v1/img/b.png)\n"
    );
}

#[test]
fn document_semantic_elements_and_roundtrip_parse() {
    let html = "<article>\
        <figure><img src=\"fig.png\" alt=\"架构\"><figcaption>图 1：系统架构</figcaption></figure>\
        <details open><summary>展开详情</summary><p>隐藏内容</p></details>\
        <dl><dt>术语</dt><dd>解释一</dd><dd><p>解释二</p></dd></dl>\
        <table><caption>表 1</caption><tr><th>列</th></tr><tr><td>值</td></tr></table>\
        <p>水 H<sub>2</sub>O，平方 x<sup>2</sup>，重点 <mark>高亮</mark>。</p>\
        <pre data-lang=\"rust\"><code>let a = 1;</code></pre><hr>\
        </article>";
    let out = from_document(html);
    assert!(out.rich);
    assert_eq!(
        out.text,
        "![架构](fig.png)\n\n*图 1：系统架构*\n\n\
         <details open>\n\n<summary>展开详情</summary>\n\n隐藏内容\n\n</details>\n\n\
         **术语**\n\n> 解释一\n\n> 解释二\n\n\
         *表 1*\n\n| 列 |\n| --- |\n| 值 |\n\n\
         水 H<sub>2</sub>O，平方 x<sup>2</sup>，重点 <mark>高亮</mark>。\n\n\
         ```rust\nlet a = 1;\n```\n\n---\n"
    );

    // 验证生成的 Markdown 可被 winc-preview::markdown::parse 正常解析为富结构。
    let parsed = crate::markdown::parse(&out.text);
    assert!(parsed.blocks.iter().any(|b| matches!(
        b,
        crate::markdown::Block::Details { open: true, .. }
    )));
    assert!(parsed.blocks.iter().any(|b| matches!(
        b,
        crate::markdown::Block::Table(_, _, true)
    )));
}
