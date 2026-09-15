//! Owned SVG/XML tree used by the optimizer.
//!
//! Intentionally simple: a recursive `Node` enum plus an `Element` with ordered
//! attributes. Reference engines use arenas / bump allocators for speed; that
//! is a later optimization, not a v1 requirement.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
    Comment(String),
    Pi {
        target: String,
        value: String,
    },
    Doctype(String),
    Decl {
        version: String,
        encoding: Option<String>,
        standalone: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// Raw tag name as parsed (`svg`, `sodipodi:namedview`, …).
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// True when the source used `<tag …/>` rather than `<tag></tag>`.
    pub self_closing: bool,
}

impl Document {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn walk_elements_mut<F>(&mut self, f: &mut F)
    where
        F: FnMut(&mut Element),
    {
        for node in &mut self.nodes {
            node.walk_elements_mut(f);
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Node {
    pub fn walk_elements_mut<F>(&mut self, f: &mut F)
    where
        F: FnMut(&mut Element),
    {
        if let Node::Element(el) = self {
            f(el);
            for child in &mut el.children {
                child.walk_elements_mut(f);
            }
        }
    }

    pub fn is_ignorable_whitespace(&self) -> bool {
        match self {
            Node::Text(t) => t.chars().all(char::is_whitespace),
            _ => false,
        }
    }
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attrs: Vec::new(),
            children: Vec::new(),
            self_closing: false,
        }
    }

    pub fn local_name(&self) -> &str {
        local_name(&self.name)
    }

    pub fn prefix(&self) -> Option<&str> {
        prefix(&self.name)
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, name: &str, value: String) {
        if let Some((_, v)) = self.attrs.iter_mut().find(|(k, _)| k == name) {
            *v = value;
        } else {
            self.attrs.push((name.to_string(), value));
        }
    }

    pub fn remove_attr(&mut self, name: &str) -> Option<String> {
        if let Some(i) = self.attrs.iter().position(|(k, _)| k == name) {
            Some(self.attrs.remove(i).1)
        } else {
            None
        }
    }

    pub fn retain_attrs<F>(&mut self, mut f: F)
    where
        F: FnMut(&str, &str) -> bool,
    {
        self.attrs.retain(|(k, v)| f(k, v));
    }
}

pub fn local_name(name: &str) -> &str {
    name.rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(name)
}

pub fn prefix(name: &str) -> Option<&str> {
    name.split_once(':').map(|(p, _)| p)
}
