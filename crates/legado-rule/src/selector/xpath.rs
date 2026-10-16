use std::ffi::c_void;
use std::sync::Once;

use libxml::bindings;
use libxml::parser::Parser;
use libxml::tree::{Document, Node, NodeType};
use libxml::xpath::Context;

use crate::content::{Content, Value};
use crate::selector::css::RuleOutput;

pub(crate) fn silence_errors() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        bindings::xmlSetStructuredErrorFunc(std::ptr::null_mut(), Some(swallow));
    });
}

unsafe extern "C" fn swallow(_ctx: *mut c_void, _error: *const bindings::xmlError) {}

pub fn eval(content: &Content, xpath: &str, output: RuleOutput) -> Result<Value, String> {
    silence_errors();
    let source = content.to_source();
    let document = parse(&source)?;

    let context = Context::new(&document).map_err(|_| "XPath 上下文创建失败".to_string())?;
    let object = context
        .evaluate_checked(xpath)
        .map_err(|error| error.to_string())?;

    let nodes = object.get_nodes_as_vec();
    if nodes.is_empty() {
        if let Some(at) = xpath.rfind("/@")
            && let Ok(parents) = context.evaluate_checked(&xpath[..at])
        {
            let count = parents.get_nodes_as_vec().len();
            if count > 0 {
                let empty = vec![String::new(); count];
                return Ok(match output {
                    RuleOutput::Elements => Value::Elements(empty),
                    RuleOutput::Text => match empty.len() {
                        1 => Value::Text(String::new()),
                        _ => Value::List(empty),
                    },
                });
            }
        }
        let text = object.to_string();
        return Ok(match text.trim() {
            "" if output == RuleOutput::Text => Value::Text(String::new()),
            "" => Value::None,
            _ => Value::Text(text),
        });
    }

    let items: Vec<String> = nodes
        .iter()
        .map(|node| node_string(&document, node))
        .collect();

    Ok(match output {
        RuleOutput::Elements => {
            if items.is_empty() {
                Value::None
            } else {
                Value::Elements(items)
            }
        }
        RuleOutput::Text => match items.len() {
            0 => Value::None,
            1 => Value::Text(items.into_iter().next().unwrap_or_default()),
            _ => Value::List(items),
        },
    })
}

fn parse(source: &str) -> Result<Document, String> {
    let mut html = source.to_string();
    if html.ends_with("</td>") {
        html = format!("<tr>{html}</tr>");
    }
    if html.ends_with("</tr>") || html.ends_with("</tbody>") {
        html = format!("<table>{html}</table>");
    }
    let html = format!("<meta charset=\"utf-8\">{html}");
    Parser::default_html()
        .parse_string(html.as_bytes())
        .map_err(|error| format!("HTML 解析失败：{error}"))
}

fn node_string(document: &Document, node: &Node) -> String {
    match node.get_type() {
        Some(NodeType::ElementNode) | Some(NodeType::HtmlDocumentNode) => {
            crate::selector::css::escape_nbsp(&crate::selector::pretty::fragment_outer_html(
                &document.node_to_string(node),
            ))
        }
        Some(NodeType::TextNode) | Some(NodeType::CDataSectionNode) => {
            crate::selector::css::normalised_text(&node.get_content())
        }
        _ => node.get_content(),
    }
}
