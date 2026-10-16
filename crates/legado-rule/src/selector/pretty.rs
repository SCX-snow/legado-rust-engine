use ego_tree::NodeRef;
use scraper::{ElementRef, Node};

struct Writer {
    out: String,
    strip_script_style: bool,
}

impl Writer {
    fn new(strip_script_style: bool) -> Self {
        Self {
            out: String::new(),
            strip_script_style,
        }
    }
}

pub(crate) fn fragment_outer_html(html: &str) -> String {
    let trimmed = html.trim_start();
    if trimmed.starts_with("<html") || trimmed.starts_with("<body") {
        let document = crate::content::parse_document(html);
        let root = document.root_element();
        let element = if trimmed.starts_with("<body") {
            root.children()
                .filter_map(ElementRef::wrap)
                .find(|element| element.value().name() == "body")
        } else {
            Some(root)
        };
        return match element {
            Some(element) => outer_html(element, false),
            None => html.to_string(),
        };
    }
    let fragment = crate::content::parse_fragment(html);
    match first_content_element(fragment.tree.root()) {
        Some(element) => outer_html(element, false),
        None => html.to_string(),
    }
}

fn first_content_element<'a>(root: NodeRef<'a, Node>) -> Option<ElementRef<'a>> {
    for child in root.children().filter_map(ElementRef::wrap) {
        match child.value().name() {
            "html" | "body" | "head" => {
                if let Some(found) = first_content_element(*child) {
                    return Some(found);
                }
            }
            _ => return Some(child),
        }
    }
    None
}

pub(crate) fn outer_html(element: ElementRef<'_>, strip_script_style: bool) -> String {
    let mut writer = Writer::new(strip_script_style);
    let node = *element;
    write_node(&mut writer, node, 0, false);
    writer.out
}

pub(crate) fn inner_html(element: ElementRef<'_>) -> String {
    let mut writer = Writer::new(false);
    let node = *element;
    for child in node.children() {
        write_node(&mut writer, child, 0, false);
    }
    writer.out.trim().to_string()
}

pub(crate) fn elements_outer_html(
    elements: &[ElementRef<'_>],
    strip_script_style: bool,
) -> Vec<String> {
    elements
        .iter()
        .map(|element| outer_html(*element, strip_script_style))
        .collect()
}

fn write_node(writer: &mut Writer, node: NodeRef<'_, Node>, depth: usize, in_raw_text: bool) {
    let out = &mut writer.out;
    match node.value() {
        Node::Element(element) => write_element(writer, node, element, depth),
        Node::Text(text) => {
            if in_raw_text {
                out.push_str(&text.text);
            } else {
                write_text(writer, &node, &text.text, depth);
            }
        }
        Node::Comment(comment) => {
            insert_indent(&node, depth, out);
            out.push_str("<!--");
            out.push_str(&comment.comment);
            out.push_str("-->");
        }
        Node::Doctype(doctype) => {
            insert_indent(&node, depth, out);
            out.push_str("<!doctype ");
            out.push_str(&doctype.name);
            out.push('>');
        }
        Node::Fragment | Node::Document => {
            for child in node.children() {
                write_node(writer, child, depth, false);
            }
        }
        _ => {}
    }
}

fn write_element(
    writer: &mut Writer,
    node: NodeRef<'_, Node>,
    element: &scraper::node::Element,
    depth: usize,
) {
    let name = element.name();
    if writer.strip_script_style && matches!(name, "script" | "style") {
        return;
    }
    let out = &mut writer.out;
    let empty = is_empty_tag(name);
    if should_indent(&node, element) && !out.is_empty() {
        indent(out, depth);
    }
    out.push('<');
    out.push_str(name);
    for (key, value) in element.attrs() {
        write_attribute(key, value, out);
    }
    out.push('>');

    let children: Vec<NodeRef<'_, Node>> = node.children().collect();
    let raw_text = matches!(name, "script" | "style");
    for child in &children {
        write_node(writer, *child, depth + 1, raw_text);
    }
    let out = &mut writer.out;
    if !(children.is_empty() && empty) {
        if !children.is_empty()
            && is_block_tag(name)
            && !formats_as_inline(name)
            && !preserve_whitespace(node.parent())
        {
            indent(out, depth);
        }
        out.push_str("</");
        out.push_str(name);
        out.push('>');
    }
}

fn write_text(writer: &mut Writer, node: &NodeRef<'_, Node>, text: &str, depth: usize) {
    let out = &mut writer.out;
    if preserve_whitespace(node.parent()) {
        escape(out, text, false, false, false, false);
        return;
    }
    let parent = node.parent();
    let parent_element = parent.as_ref().and_then(|parent| ElementRef::wrap(*parent));
    let trim_like_block = parent_element
        .map(|element| {
            let name = element.value().name();
            is_block_tag(name) || !formats_as_inline(name)
        })
        .unwrap_or(false);
    let trim_leading = (trim_like_block && sibling_index(node) == 0)
        || parent
            .as_ref()
            .map(|parent| matches!(parent.value(), Node::Document))
            .unwrap_or(false);
    let trim_trailing = trim_like_block && node.next_sibling().is_none();
    let blank = is_blank(text);
    let could_skip = next_sibling_will_indent(node)
        || next_sibling_is_blank_text(node)
        || previous_sibling_is_block_or_br(node);
    if could_skip && blank {
        return;
    }
    let format_as_block_parent = parent_element
        .map(|element| !formats_as_inline(element.value().name()))
        .unwrap_or(false);
    if (sibling_index(node) == 0 && format_as_block_parent && !blank)
        || previous_sibling_is_br(node)
    {
        indent(out, depth);
    }
    escape(out, text, false, true, trim_leading, trim_trailing);
}

fn should_indent(node: &NodeRef<'_, Node>, element: &scraper::node::Element) -> bool {
    let name = element.name();
    let format_as_block = is_block_tag(name)
        || node
            .parent()
            .as_ref()
            .and_then(|node| ElementRef::wrap(*node))
            .map(|parent| !formats_as_inline(parent.value().name()))
            .unwrap_or(false);
    if !format_as_block {
        return false;
    }
    let inlineable = !is_block_tag(name)
        && node
            .parent()
            .as_ref()
            .and_then(|node| ElementRef::wrap(*node))
            .map(|parent| is_block_tag(parent.value().name()))
            .unwrap_or(true)
        && sibling_index(node) != 0
        && name != "br";
    !inlineable && !preserve_whitespace(node.parent())
}

fn preserve_whitespace(node: Option<NodeRef<'_, Node>>) -> bool {
    let mut current = node;
    let mut depth = 0;
    while let Some(node) = current {
        if let Node::Element(element) = node.value()
            && PRESERVE_WHITESPACE_TAGS.contains(&element.name())
        {
            return true;
        }
        current = node.parent();
        depth += 1;
        if depth >= 6 {
            break;
        }
    }
    false
}

const BLOCK_TAGS: &[&str] = &[
    "address",
    "applet",
    "article",
    "aside",
    "audio",
    "blockquote",
    "body",
    "canvas",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "ins",
    "li",
    "link",
    "listing",
    "main",
    "marquee",
    "math",
    "menu",
    "meta",
    "nav",
    "noframes",
    "noscript",
    "ol",
    "p",
    "plaintext",
    "pre",
    "script",
    "section",
    "style",
    "svg",
    "table",
    "tbody",
    "td",
    "template",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "ul",
    "video",
];
const FORMAT_AS_INLINE_TAGS: &[&str] = &[
    "a", "abbr", "acronym", "address", "area", "b", "base", "basefont", "bdi", "bdo", "bgsound",
    "big", "br", "button", "cite", "code", "command", "data", "datalist", "del", "device", "dfn",
    "em", "embed", "font", "h1", "h2", "h3", "h4", "h5", "h6", "i", "iframe", "img", "input",
    "ins", "kbd", "keygen", "label", "legend", "li", "map", "mark", "menuitem", "meter", "mi",
    "mn", "mo", "msup", "mtext", "nobr", "object", "optgroup", "option", "output", "p", "param",
    "pre", "progress", "q", "rb", "rp", "rt", "rtc", "ruby", "s", "samp", "script", "select",
    "small", "source", "span", "strike", "strong", "style", "sub", "summary", "sup", "td", "text",
    "textarea", "th", "time", "title", "track", "tt", "u", "var", "wbr",
];
const PRESERVE_WHITESPACE_TAGS: &[&str] = &["plaintext", "pre", "textarea", "title"];
const EMPTY_TAGS: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "command", "device", "embed", "frame",
    "hr", "img", "input", "keygen", "link", "menuitem", "meta", "param", "source", "track", "wbr",
];
fn is_block_tag(name: &str) -> bool {
    BLOCK_TAGS.contains(&name)
}

pub(crate) fn formats_as_inline(name: &str) -> bool {
    FORMAT_AS_INLINE_TAGS.contains(&name)
}

pub(crate) fn is_block(name: &str) -> bool {
    BLOCK_TAGS.contains(&name)
}

fn is_empty_tag(name: &str) -> bool {
    EMPTY_TAGS.contains(&name)
}

fn indent(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push(' ');
    }
}

fn insert_indent(node: &NodeRef<'_, Node>, depth: usize, out: &mut String) {
    if !out.is_empty() && should_indent_comment(node) {
        indent(out, depth);
    }
}

fn should_indent_comment(node: &NodeRef<'_, Node>) -> bool {
    node.parent()
        .as_ref()
        .and_then(|node| ElementRef::wrap(*node))
        .map(|parent| is_block_tag(parent.value().name()))
        .unwrap_or(false)
}

fn escape(
    out: &mut String,
    text: &str,
    in_attribute: bool,
    normalise_white: bool,
    strip_leading: bool,
    trim_trailing: bool,
) {
    let mut last_was_white = false;
    let mut reached_non_white = false;
    let mut skipped = false;
    for ch in text.chars() {
        if normalise_white {
            if matches!(ch, ' ' | '\t' | '\n' | '\u{c}' | '\r') {
                if strip_leading && !reached_non_white {
                    continue;
                }
                if last_was_white {
                    continue;
                }
                if trim_trailing {
                    skipped = true;
                    continue;
                }
                out.push(' ');
                last_was_white = true;
                continue;
            }
            last_was_white = false;
            reached_non_white = true;
            if skipped {
                out.push(' ');
                skipped = false;
            }
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '<' if !in_attribute => out.push_str("&lt;"),
            '>' if !in_attribute => out.push_str("&gt;"),
            '"' if in_attribute => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(ch),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("&#x{:x};", ch as u32)),
            ch => out.push(ch),
        }
    }
}

fn write_attribute(key: &str, value: &str, out: &mut String) {
    out.push(' ');
    out.push_str(key);
    if (value.is_empty() || value.eq_ignore_ascii_case(key)) && is_boolean_attribute(key) {
        return;
    }
    out.push_str("=\"");
    escape(out, value, true, false, false, false);
    out.push('"');
}

fn is_boolean_attribute(key: &str) -> bool {
    const BOOLEAN: &[&str] = &[
        "allowfullscreen",
        "async",
        "autofocus",
        "checked",
        "compact",
        "declare",
        "default",
        "defaultchecked",
        "defaultmuted",
        "defaultselected",
        "defer",
        "disabled",
        "formnovalidate",
        "hidden",
        "indeterminate",
        "inert",
        "ismap",
        "itemscope",
        "loop",
        "multiple",
        "muted",
        "nohref",
        "noresize",
        "noshade",
        "novalidate",
        "nowrap",
        "open",
        "pauseonexit",
        "readonly",
        "required",
        "reversed",
        "scoped",
        "seamless",
        "selected",
        "sortable",
        "truespeed",
        "typemustmatch",
        "visible",
    ];
    BOOLEAN.contains(&key.to_ascii_lowercase().as_str())
}

fn is_blank(text: &str) -> bool {
    text.chars()
        .all(|ch| matches!(ch, ' ' | '\t' | '\n' | '\u{c}' | '\r'))
}

fn sibling_index(node: &NodeRef<'_, Node>) -> usize {
    node.prev_siblings().count()
}

fn next_sibling_will_indent(node: &NodeRef<'_, Node>) -> bool {
    match node.next_sibling() {
        Some(next) => match next.value() {
            Node::Element(element) => should_indent(&next, element),
            _ => false,
        },
        None => false,
    }
}

fn next_sibling_is_blank_text(node: &NodeRef<'_, Node>) -> bool {
    match node.next_sibling() {
        Some(next) => match next.value() {
            Node::Text(text) => is_blank(&text.text),
            _ => false,
        },
        None => false,
    }
}

fn previous_sibling_is_block_or_br(node: &NodeRef<'_, Node>) -> bool {
    match node.prev_sibling() {
        Some(prev) => match prev.value() {
            Node::Element(element) => {
                let name = element.name();
                is_block_tag(name) || name == "br"
            }
            _ => false,
        },
        None => false,
    }
}

fn previous_sibling_is_br(node: &NodeRef<'_, Node>) -> bool {
    match node.prev_sibling() {
        Some(prev) => match prev.value() {
            Node::Element(element) => element.name() == "br",
            _ => false,
        },
        None => false,
    }
}
