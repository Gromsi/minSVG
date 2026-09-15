use crate::ast::{Document, Element, Node};
use crate::OptimizeError;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

pub fn parse_svg(input: &str) -> Result<Document, OptimizeError> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = false;

    let mut doc = Document::new();
    let mut stack: Vec<Element> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let el = element_from_start(&e, &reader, false)?;
                stack.push(el);
            }
            Ok(Event::Empty(e)) => {
                let el = element_from_start(&e, &reader, true)?;
                push_node(&mut doc, &mut stack, Node::Element(el));
            }
            Ok(Event::End(_)) => {
                let el = stack
                    .pop()
                    .ok_or_else(|| OptimizeError::Parse("unexpected closing tag".to_string()))?;
                push_node(&mut doc, &mut stack, Node::Element(el));
            }
            Ok(Event::Text(t)) => {
                let text = t
                    .unescape()
                    .map_err(|e| OptimizeError::Parse(e.to_string()))?
                    .into_owned();
                if !text.is_empty() {
                    push_node(&mut doc, &mut stack, Node::Text(text));
                }
            }
            Ok(Event::CData(t)) => {
                let text = String::from_utf8_lossy(&t).into_owned();
                push_node(&mut doc, &mut stack, Node::Text(text));
            }
            Ok(Event::Comment(c)) => {
                let text = String::from_utf8_lossy(&c).into_owned();
                push_node(&mut doc, &mut stack, Node::Comment(text));
            }
            Ok(Event::DocType(d)) => {
                let text = String::from_utf8_lossy(&d).into_owned();
                push_node(&mut doc, &mut stack, Node::Doctype(text));
            }
            Ok(Event::PI(p)) => {
                let raw = String::from_utf8_lossy(&p).into_owned();
                let (target, value) = split_pi(&raw);
                push_node(&mut doc, &mut stack, Node::Pi { target, value });
            }
            Ok(Event::Decl(d)) => {
                let version = d
                    .version()
                    .map(|v| String::from_utf8_lossy(&v).into_owned())
                    .unwrap_or_else(|_| "1.0".to_string());
                let encoding = d
                    .encoding()
                    .and_then(|r| r.ok())
                    .map(|v| String::from_utf8_lossy(&v).into_owned());
                let standalone = d
                    .standalone()
                    .and_then(|r| r.ok())
                    .map(|v| String::from_utf8_lossy(&v).into_owned());
                push_node(
                    &mut doc,
                    &mut stack,
                    Node::Decl {
                        version,
                        encoding,
                        standalone,
                    },
                );
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OptimizeError::Parse(format!(
                    "XML parse error at position {}: {e}",
                    reader.buffer_position()
                )));
            }
        }
        buf.clear();
    }

    if !stack.is_empty() {
        return Err(OptimizeError::Parse(
            "unclosed element at end of document".to_string(),
        ));
    }

    Ok(doc)
}

fn element_from_start(
    start: &BytesStart<'_>,
    reader: &Reader<&[u8]>,
    self_closing: bool,
) -> Result<Element, OptimizeError> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut el = Element::new(name);
    el.self_closing = self_closing;
    for attr in start.attributes() {
        let attr = attr.map_err(|e| OptimizeError::Parse(e.to_string()))?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        let value = attr
            .decode_and_unescape_value(reader.decoder())
            .map_err(|e| OptimizeError::Parse(e.to_string()))?
            .into_owned();
        el.attrs.push((key, value));
    }
    Ok(el)
}

fn push_node(doc: &mut Document, stack: &mut [Element], node: Node) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        doc.nodes.push(node);
    }
}

fn split_pi(raw: &str) -> (String, String) {
    let raw = raw.trim();
    match raw.split_once(|c: char| c.is_whitespace()) {
        Some((target, rest)) => (target.to_string(), rest.trim().to_string()),
        None => (raw.to_string(), String::new()),
    }
}
