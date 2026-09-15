//! Opt-in edit plugins (SVGO-inspired names, clean-room, **default OFF**).
//!
//! These are injectors / user-configured deletions, not minifiers. They are
//! **not** in [`crate::DEFAULT_PLUGIN_NAMES`] and must be called with params.
//! Empty params are a no-op.
//!
//! Public contracts only — not a port of SVGO / oxvg / vexy source:
//!
//! - [addAttributesToSVGElement](https://svgo.dev/docs/plugins/addAttributesToSVGElement/)
//!   (`params`: `attribute` string/object or `attributes` array/object)
//! - [addClassesToSVGElement](https://svgo.dev/docs/plugins/addClassesToSVGElement/)
//!   (`params`: `className` / `classNames`)
//! - [removeAttrs](https://svgo.dev/docs/plugins/removeAttrs/)
//!   (`params`: `attrs`, `elemSeparator`, `preserveCurrentColor`)
//! - [removeAttributesBySelector](https://svgo.dev/docs/plugins/removeAttributesBySelector/)
//!   (simple `tag` / `.class` / `#id` selectors — not a CSS engine)
//! - [removeElementsByAttr](https://svgo.dev/docs/plugins/removeElementsByAttr/)
//!   (`id` / `class` / `data`)
//!
//! Merge-wire (`plugins.rs`) stays untouched: do **not** add these IDs to the
//! default pipeline.

use crate::ast::{local_name, Document, Element, Node};
use crate::config::Config;
use serde_json::Value;

/// SVGO built-in IDs for this opt-in subset. All stay **off** by default.
pub const OPTIN_EDIT_PLUGIN_NAMES: &[&str] = &[
    "addAttributesToSVGElement",
    "addClassesToSVGElement",
    "removeAttrs",
    "removeAttributesBySelector",
    "removeElementsByAttr",
];

const DEFAULT_ELEM_SEPARATOR: &str = ":";

// ---------------------------------------------------------------------------
// addAttributesToSVGElement
// ---------------------------------------------------------------------------

/// `addAttributesToSVGElement` params: a map of attribute names → values.
///
/// Only key/value pairs (the safe public form). Existing keys on the
/// outermost `<svg>` are left alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddAttributesToSvgElementParams {
    pub attributes: Vec<(String, String)>,
}

impl AddAttributesToSvgElementParams {
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            attributes: pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// Parse the public SVGO object:
    /// `{ attribute: "name" | { k: v } }` or
    /// `{ attributes: ["name", { k: v }, …] | { k: v } }`.
    /// A bare string / array / map is accepted as the same payload.
    pub fn from_value(value: &Value) -> Self {
        let mut attributes = Vec::new();
        if let Some(obj) = value.as_object() {
            if let Some(attrs) = obj.get("attributes") {
                collect_attr_object(attrs, &mut attributes);
            }
            if attributes.is_empty() {
                if let Some(attr) = obj.get("attribute") {
                    collect_attr_object(attr, &mut attributes);
                }
            }
            if attributes.is_empty()
                && !obj.contains_key("attributes")
                && !obj.contains_key("attribute")
            {
                collect_attr_object(value, &mut attributes);
            }
        } else {
            collect_attr_object(value, &mut attributes);
        }
        Self { attributes }
    }

    pub fn from_config(config: &Config) -> Self {
        config
            .plugin_param("addAttributesToSVGElement")
            .map(Self::from_value)
            .unwrap_or_default()
    }
}

/// Add attributes to each outermost `<svg>` when the key is absent.
///
/// Nested `<svg>` is ignored. Empty [`AddAttributesToSvgElementParams::attributes`]
/// is a no-op.
pub fn run_add_attributes_to_svg_element(
    doc: &mut Document,
    params: &AddAttributesToSvgElementParams,
) {
    if params.attributes.is_empty() {
        return;
    }
    for_each_root_svg(doc, |el| {
        for (name, value) in &params.attributes {
            if name.is_empty() {
                continue;
            }
            if el.attr(name).is_none() {
                el.set_attr(name, value.clone());
            }
        }
    });
}

// ---------------------------------------------------------------------------
// addClassesToSVGElement
// ---------------------------------------------------------------------------

/// `addClassesToSVGElement` params: one `className` and/or a `classNames` list.
///
/// When `class_names` is non-empty it wins (SVGO: `classNames` overrides
/// `className`). Classes are **added**; existing tokens stay; duplicates drop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddClassesToSvgElementParams {
    pub class_name: Option<String>,
    pub class_names: Vec<String>,
}

impl AddClassesToSvgElementParams {
    pub fn from_class_name(name: impl Into<String>) -> Self {
        Self {
            class_name: Some(name.into()),
            class_names: Vec::new(),
        }
    }

    pub fn from_class_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            class_name: None,
            class_names: names.into_iter().map(Into::into).collect(),
        }
    }

    fn resolved_class_names(&self) -> Vec<&str> {
        if !self.class_names.is_empty() {
            self.class_names.iter().map(String::as_str).collect()
        } else if let Some(name) = self.class_name.as_deref() {
            vec![name]
        } else {
            Vec::new()
        }
    }

    /// Parse `{ className, classNames }` (SVGO). `classNames` wins when
    /// non-empty. A bare string or array is accepted.
    pub fn from_value(value: &Value) -> Self {
        let mut p = Self::default();
        match value {
            Value::String(s) => {
                p.class_name = Some(s.clone());
            }
            Value::Array(_) => {
                p.class_names = json_string_list(value);
            }
            Value::Object(obj) => {
                if let Some(names) = obj.get("classNames") {
                    p.class_names = json_string_list(names);
                }
                if let Some(name) = obj.get("className") {
                    match name {
                        Value::String(s) => p.class_name = Some(s.clone()),
                        Value::Array(_) => {
                            let extra = json_string_list(name);
                            if p.class_names.is_empty() {
                                p.class_names = extra;
                            } else if p.class_name.is_none() {
                                p.class_name = extra.into_iter().next();
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        p
    }

    pub fn from_config(config: &Config) -> Self {
        config
            .plugin_param("addClassesToSVGElement")
            .map(Self::from_value)
            .unwrap_or_default()
    }
}

/// Add class tokens to each outermost `<svg>`. Nested `<svg>` is ignored.
pub fn run_add_classes_to_svg_element(doc: &mut Document, params: &AddClassesToSvgElementParams) {
    let extra = params.resolved_class_names();
    if extra.is_empty() {
        return;
    }
    for_each_root_svg(doc, |el| {
        let existing = el.attr("class").unwrap_or("").to_string();
        let mut tokens: Vec<String> = class_tokens(&existing)
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut changed = false;
        for raw in &extra {
            let name = raw.trim();
            if name.is_empty() {
                continue;
            }
            if !tokens.iter().any(|t| t == name) {
                tokens.push(name.to_string());
                changed = true;
            }
        }
        if changed && !tokens.is_empty() {
            el.set_attr("class", tokens.join(" "));
        }
    });
}

// ---------------------------------------------------------------------------
// removeAttrs
// ---------------------------------------------------------------------------

/// `removeAttrs` params: a glob list (`fill`, `stroke-*`, `path:fill:none`).
///
/// Pattern form is `[element : attribute : value]` with [`Self::elem_separator`]
/// (default `:`). A bare token is an attribute glob on every element.
/// `*` / `?` are globs (not regex). `xml:` / `xlink:` / `xmlns:` names stay
/// one attribute when the default separator is `:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveAttrsParams {
    pub attrs: Vec<String>,
    pub elem_separator: String,
    pub preserve_current_color: bool,
}

impl Default for RemoveAttrsParams {
    fn default() -> Self {
        Self {
            attrs: Vec::new(),
            elem_separator: DEFAULT_ELEM_SEPARATOR.to_string(),
            preserve_current_color: false,
        }
    }
}

impl RemoveAttrsParams {
    pub fn from_attrs<I, S>(attrs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            attrs: attrs.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    pub fn from_value(value: &Value) -> Self {
        let mut p = Self::default();
        let Some(obj) = value.as_object() else {
            p.attrs = json_string_list(value);
            return p;
        };
        if let Some(attrs) = obj.get("attrs") {
            p.attrs = json_string_list(attrs);
        }
        if let Some(sep) = obj.get("elemSeparator").and_then(Value::as_str) {
            p.elem_separator = sep.to_string();
        }
        if let Some(b) = obj.get("preserveCurrentColor").and_then(Value::as_bool) {
            p.preserve_current_color = b;
        }
        p
    }

    pub fn from_config(config: &Config) -> Self {
        config
            .plugin_param("removeAttrs")
            .map(Self::from_value)
            .unwrap_or_default()
    }
}

/// Drop attributes whose element / name / value match a glob in `params.attrs`.
pub fn run_remove_attrs(doc: &mut Document, params: &RemoveAttrsParams) {
    if params.attrs.is_empty() {
        return;
    }
    let sep = params.elem_separator.as_str();
    let globs: Vec<AttrGlob> = params
        .attrs
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| parse_attr_glob(p, sep))
        .collect();
    if globs.is_empty() {
        return;
    }
    let preserve = params.preserve_current_color;
    doc.walk_elements_mut(&mut |el| {
        let local = el.local_name().to_ascii_lowercase();
        let qname = el.name.to_ascii_lowercase();
        el.retain_attrs(|name, value| {
            if preserve && is_fill_or_stroke(name) && is_current_color(value) {
                return true;
            }
            !globs.iter().any(|g| g.matches(&local, &qname, name, value))
        });
    });
}

// ---------------------------------------------------------------------------
// removeAttributesBySelector
// ---------------------------------------------------------------------------

/// One `selector` + attribute list for [`RemoveAttributesBySelectorParams`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectorAttrRule {
    pub selector: String,
    pub attributes: Vec<String>,
}

impl SelectorAttrRule {
    pub fn new(
        selector: impl Into<String>,
        attributes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            selector: selector.into(),
            attributes: attributes.into_iter().map(Into::into).collect(),
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        let obj = value.as_object()?;
        let selector = obj.get("selector").and_then(Value::as_str)?.to_string();
        let attributes = obj
            .get("attributes")
            .map(json_string_list)
            .unwrap_or_default();
        if selector.is_empty() || attributes.is_empty() {
            return None;
        }
        Some(Self {
            selector,
            attributes,
        })
    }
}

/// `removeAttributesBySelector` params: simple tag / class / id selectors.
///
/// Supported: `*`, `circle`, `#logo`, `.icon`, `circle.icon`, `path#p.x`,
/// comma lists (`circle, rect`). No combinators, attribute selectors, or
/// pseudos — those fail closed (no match).
///
/// When [`Self::selectors`] is non-empty it is used; otherwise `selector` +
/// `attributes`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoveAttributesBySelectorParams {
    pub selector: Option<String>,
    pub attributes: Vec<String>,
    pub selectors: Vec<SelectorAttrRule>,
}

impl RemoveAttributesBySelectorParams {
    pub fn single(
        selector: impl Into<String>,
        attributes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            selector: Some(selector.into()),
            attributes: attributes.into_iter().map(Into::into).collect(),
            selectors: Vec::new(),
        }
    }

    fn rules(&self) -> Vec<SelectorAttrRule> {
        if !self.selectors.is_empty() {
            return self.selectors.clone();
        }
        match self.selector.as_deref() {
            Some(sel) if !sel.is_empty() && !self.attributes.is_empty() => {
                vec![SelectorAttrRule {
                    selector: sel.to_string(),
                    attributes: self.attributes.clone(),
                }]
            }
            _ => Vec::new(),
        }
    }

    pub fn from_value(value: &Value) -> Self {
        let mut p = Self::default();
        let Some(obj) = value.as_object() else {
            return p;
        };
        if let Some(sel) = obj.get("selector").and_then(Value::as_str) {
            p.selector = Some(sel.to_string());
        }
        if let Some(attrs) = obj.get("attributes") {
            p.attributes = json_string_list(attrs);
        }
        if let Some(Value::Array(items)) = obj.get("selectors") {
            p.selectors = items
                .iter()
                .filter_map(SelectorAttrRule::from_value)
                .collect();
        }
        p
    }

    pub fn from_config(config: &Config) -> Self {
        config
            .plugin_param("removeAttributesBySelector")
            .map(Self::from_value)
            .unwrap_or_default()
    }
}

/// Remove named attributes from elements that match a simple selector.
pub fn run_remove_attributes_by_selector(
    doc: &mut Document,
    params: &RemoveAttributesBySelectorParams,
) {
    let rules = params.rules();
    if rules.is_empty() {
        return;
    }
    let compiled: Vec<(Vec<SimpleSelector>, Vec<String>)> = rules
        .iter()
        .filter(|r| !r.attributes.is_empty())
        .map(|r| (parse_selector_list(&r.selector), r.attributes.clone()))
        .filter(|(sels, _)| !sels.is_empty())
        .collect();
    if compiled.is_empty() {
        return;
    }
    doc.walk_elements_mut(&mut |el| {
        for (sels, attrs) in &compiled {
            if sels.iter().any(|sel| selector_matches(el, sel)) {
                for name in attrs {
                    el.remove_attr(name);
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// removeElementsByAttr
// ---------------------------------------------------------------------------

/// `removeElementsByAttr` params: drop elements by `id`, `class`, or `data`.
///
/// `data` entries are `foo` / `data-foo` (presence) or `foo=bar` (value).
/// `data-*` matches any `data-*` attribute. The outermost `<svg>` is never
/// removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoveElementsByAttrParams {
    pub id: Vec<String>,
    pub class: Vec<String>,
    pub data: Vec<String>,
}

impl RemoveElementsByAttrParams {
    pub fn from_ids<I, S>(ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            id: ids.into_iter().map(Into::into).collect(),
            class: Vec::new(),
            data: Vec::new(),
        }
    }

    pub fn from_classes<I, S>(classes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            id: Vec::new(),
            class: classes.into_iter().map(Into::into).collect(),
            data: Vec::new(),
        }
    }

    pub fn from_data<I, S>(data: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            id: Vec::new(),
            class: Vec::new(),
            data: data.into_iter().map(Into::into).collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.id.is_empty() && self.class.is_empty() && self.data.is_empty()
    }

    pub fn from_value(value: &Value) -> Self {
        let mut p = Self::default();
        let Some(obj) = value.as_object() else {
            return p;
        };
        if let Some(id) = obj.get("id") {
            p.id = json_string_list(id);
        }
        if let Some(class) = obj.get("class") {
            p.class = json_string_list(class);
        }
        if let Some(data) = obj.get("data") {
            p.data = json_string_list(data);
        }
        p
    }

    pub fn from_config(config: &Config) -> Self {
        config
            .plugin_param("removeElementsByAttr")
            .map(Self::from_value)
            .unwrap_or_default()
    }
}

/// Run opt-in edit plugins that [`Config::plugin_enabled`] turned on.
///
/// Default [`Config`] enables none of them. Removals run first so later
/// default passes do not see deleted nodes; injectors run last in this set.
pub fn run_enabled_edit_pass(doc: &mut Document, config: &Config) -> bool {
    let mut ran = false;
    if config.plugin_enabled("removeElementsByAttr") {
        run_remove_elements_by_attr(doc, &RemoveElementsByAttrParams::from_config(config));
        ran = true;
    }
    if config.plugin_enabled("removeAttrs") {
        run_remove_attrs(doc, &RemoveAttrsParams::from_config(config));
        ran = true;
    }
    if config.plugin_enabled("removeAttributesBySelector") {
        run_remove_attributes_by_selector(
            doc,
            &RemoveAttributesBySelectorParams::from_config(config),
        );
        ran = true;
    }
    if config.plugin_enabled("addAttributesToSVGElement") {
        run_add_attributes_to_svg_element(
            doc,
            &AddAttributesToSvgElementParams::from_config(config),
        );
        ran = true;
    }
    if config.plugin_enabled("addClassesToSVGElement") {
        run_add_classes_to_svg_element(doc, &AddClassesToSvgElementParams::from_config(config));
        ran = true;
    }
    ran
}

/// Remove matching elements (and their subtrees). Root `<svg>` stays.
pub fn run_remove_elements_by_attr(doc: &mut Document, params: &RemoveElementsByAttrParams) {
    if params.is_empty() {
        return;
    }
    drop_matching_elements(&mut doc.nodes, params, true);
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn for_each_root_svg<F>(doc: &mut Document, mut f: F)
where
    F: FnMut(&mut Element),
{
    for node in &mut doc.nodes {
        if let Node::Element(el) = node {
            if el.local_name().eq_ignore_ascii_case("svg") {
                f(el);
            }
        }
    }
}

fn json_string_list(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => {
            if s.is_empty() {
                Vec::new()
            } else {
                vec![s.clone()]
            }
        }
        Value::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn collect_attr_object(value: &Value, out: &mut Vec<(String, String)>) {
    match value {
        Value::String(s) => {
            if !s.is_empty() {
                out.push((s.clone(), String::new()));
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                if k.is_empty() {
                    continue;
                }
                let s = match v {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => continue,
                };
                out.push((k.clone(), s));
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_attr_object(item, out);
            }
        }
        _ => {}
    }
}

fn class_tokens(value: &str) -> Vec<&str> {
    value
        .split(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r'))
        .filter(|t| !t.is_empty())
        .collect()
}

fn is_current_color(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("currentcolor")
}

fn is_fill_or_stroke(name: &str) -> bool {
    let local = local_name(name);
    local.eq_ignore_ascii_case("fill") || local.eq_ignore_ascii_case("stroke")
}

fn is_attr_namespace_prefix(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "xml" | "xmlns" | "xlink")
}

#[derive(Debug, Clone)]
struct AttrGlob {
    elem: String,
    attr: String,
    value: String,
}

impl AttrGlob {
    fn matches(&self, local: &str, qname: &str, name: &str, value: &str) -> bool {
        let elem_ok = glob_ci(&self.elem, local) || glob_ci(&self.elem, qname);
        let attr_ok = glob_ci(&self.attr, name) || glob_ci(&self.attr, local_name(name));
        elem_ok && attr_ok && glob_ci(&self.value, value)
    }
}

fn parse_attr_glob(pattern: &str, sep: &str) -> AttrGlob {
    if sep.is_empty() || !pattern.contains(sep) {
        return AttrGlob {
            elem: "*".into(),
            attr: pattern.to_string(),
            value: "*".into(),
        };
    }
    let parts: Vec<&str> = pattern.split(sep).collect();
    let (elem, attr, value) = match parts.as_slice() {
        [] => ("*".to_string(), String::new(), "*".to_string()),
        [attr] => ("*".to_string(), (*attr).to_string(), "*".to_string()),
        [prefix, local] if sep == ":" && is_attr_namespace_prefix(prefix) => (
            "*".to_string(),
            format!("{prefix}:{local}"),
            "*".to_string(),
        ),
        [elem, attr] => ((*elem).to_string(), (*attr).to_string(), "*".to_string()),
        [elem, prefix, local] if sep == ":" && is_attr_namespace_prefix(prefix) => (
            (*elem).to_string(),
            format!("{prefix}:{local}"),
            "*".to_string(),
        ),
        [elem, prefix, local, value] if sep == ":" && is_attr_namespace_prefix(prefix) => (
            (*elem).to_string(),
            format!("{prefix}:{local}"),
            (*value).to_string(),
        ),
        [elem, attr, value] => (
            (*elem).to_string(),
            (*attr).to_string(),
            (*value).to_string(),
        ),
        [elem, rest @ ..] if rest.len() >= 2 => {
            let value = rest[rest.len() - 1].to_string();
            let attr = rest[..rest.len() - 1].join(sep);
            ((*elem).to_string(), attr, value)
        }
        _ => ("*".to_string(), pattern.to_string(), "*".to_string()),
    };
    AttrGlob { elem, attr, value }
}

fn glob_ci(pat: &str, text: &str) -> bool {
    if pat == "*" {
        return true;
    }
    glob_match(&pat.to_ascii_lowercase(), &text.to_ascii_lowercase())
}

fn glob_match(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_rec(&p, &t)
}

fn glob_rec(pat: &[char], text: &[char]) -> bool {
    match pat.first() {
        None => text.is_empty(),
        Some('*') => {
            let mut i = 0;
            while i < pat.len() && pat[i] == '*' {
                i += 1;
            }
            let rest = &pat[i..];
            if rest.is_empty() {
                return true;
            }
            for skip in 0..=text.len() {
                if glob_rec(rest, &text[skip..]) {
                    return true;
                }
            }
            false
        }
        Some('?') => !text.is_empty() && glob_rec(&pat[1..], &text[1..]),
        Some(c) => text.first() == Some(c) && glob_rec(&pat[1..], &text[1..]),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SimpleSelector {
    /// `None` or `*` → any tag.
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

fn parse_selector_list(raw: &str) -> Vec<SimpleSelector> {
    raw.split(',')
        .filter_map(|part| parse_one_selector(part.trim()))
        .collect()
}

fn parse_one_selector(raw: &str) -> Option<SimpleSelector> {
    if raw.is_empty() {
        return None;
    }
    if raw.chars().any(|c| {
        matches!(
            c,
            ' ' | '\t' | '\n' | '\r' | '>' | '+' | '~' | '[' | ']' | ':' | '(' | ')' | '"' | '\''
        )
    }) {
        return None;
    }
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    let mut tag = None;
    let mut id = None;
    let mut classes = Vec::new();
    if !matches!(chars.first(), Some('#') | Some('.')) {
        if chars.first() == Some(&'*') {
            tag = Some("*".into());
            i = 1;
        } else {
            let start = i;
            while i < chars.len() && is_ident_char(chars[i]) {
                i += 1;
            }
            if i == start {
                return None;
            }
            tag = Some(chars[start..i].iter().collect());
        }
    }
    while i < chars.len() {
        match chars[i] {
            '#' => {
                i += 1;
                let start = i;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                if i == start || id.is_some() {
                    return None;
                }
                id = Some(chars[start..i].iter().collect());
            }
            '.' => {
                i += 1;
                let start = i;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                if i == start {
                    return None;
                }
                classes.push(chars[start..i].iter().collect());
            }
            _ => return None,
        }
    }
    Some(SimpleSelector { tag, id, classes })
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn selector_matches(el: &Element, sel: &SimpleSelector) -> bool {
    if let Some(tag) = &sel.tag {
        if tag != "*"
            && !el.local_name().eq_ignore_ascii_case(tag)
            && !el.name.eq_ignore_ascii_case(tag)
        {
            return false;
        }
    }
    if let Some(want) = &sel.id {
        let got = el.attr("id").or_else(|| el.attr("xml:id"));
        if got != Some(want.as_str()) {
            return false;
        }
    }
    if !sel.classes.is_empty() {
        let have = class_tokens(el.attr("class").unwrap_or(""));
        for class in &sel.classes {
            if !have.iter().any(|h| *h == class.as_str()) {
                return false;
            }
        }
    }
    true
}

fn drop_matching_elements(
    nodes: &mut Vec<Node>,
    params: &RemoveElementsByAttrParams,
    at_root: bool,
) {
    nodes.retain(|node| match node {
        Node::Element(el) => {
            let root_svg = at_root && el.local_name().eq_ignore_ascii_case("svg");
            root_svg || !element_matches_remove(el, params)
        }
        _ => true,
    });
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            drop_matching_elements(&mut el.children, params, false);
        }
    }
}

fn element_matches_remove(el: &Element, params: &RemoveElementsByAttrParams) -> bool {
    if !params.id.is_empty() {
        if let Some(id) = el.attr("id").or_else(|| el.attr("xml:id")) {
            if params.id.iter().any(|w| w == id) {
                return true;
            }
        }
    }
    if !params.class.is_empty() {
        let have = class_tokens(el.attr("class").unwrap_or(""));
        if params
            .class
            .iter()
            .any(|want| have.iter().any(|h| *h == want.as_str()))
        {
            return true;
        }
    }
    if !params.data.is_empty() {
        if params.data.iter().any(|spec| data_attr_matches(el, spec)) {
            return true;
        }
    }
    false
}

fn data_attr_matches(el: &Element, spec: &str) -> bool {
    let spec = spec.trim();
    if spec.is_empty() {
        return false;
    }
    let (raw_name, want) = match spec.split_once('=') {
        Some((n, v)) => (n.trim(), Some(v)),
        None => (spec, None),
    };
    if raw_name.is_empty() {
        return false;
    }
    let any_data = raw_name == "*" || raw_name.eq_ignore_ascii_case("data-*");
    let name_pat = if any_data {
        None
    } else if raw_name.starts_with("data-") || raw_name.starts_with("data:") {
        Some(raw_name.to_string())
    } else {
        Some(format!("data-{raw_name}"))
    };
    el.attrs.iter().any(|(k, v)| {
        let is_data = k.starts_with("data-") || k.starts_with("data:");
        if !is_data {
            return false;
        }
        let name_ok = match name_pat.as_deref() {
            None => true,
            Some(pat) => glob_ci(pat, k),
        };
        let value_ok = match want {
            None => true,
            Some(w) => v == w,
        };
        name_ok && value_ok
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_svg;
    use crate::serialize::serialize;
    use crate::DEFAULT_PLUGIN_NAMES;

    fn parse(s: &str) -> Document {
        parse_svg(s).unwrap()
    }

    fn emit(doc: &Document) -> String {
        serialize(doc)
    }

    fn sample() -> &'static str {
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" class="root" role="img">"##,
            r##"<g id="keep"><rect id="box" class="icon x" fill="red" stroke-width="2" data-k="1"/>"##,
            r##"<path id="line" fill="none" stroke="currentColor" d="M0 0h2"/>"##,
            r##"<svg class="nested" id="inner"><circle fill="blue"/></svg>"##,
            "</g></svg>",
        )
    }

    #[test]
    fn names_are_svgo_ids_and_stay_off_default() {
        assert_eq!(
            OPTIN_EDIT_PLUGIN_NAMES,
            &[
                "addAttributesToSVGElement",
                "addClassesToSVGElement",
                "removeAttrs",
                "removeAttributesBySelector",
                "removeElementsByAttr",
            ]
        );
        for name in OPTIN_EDIT_PLUGIN_NAMES {
            assert!(
                !DEFAULT_PLUGIN_NAMES.contains(name),
                "{name} must stay opt-in / default OFF"
            );
        }
    }

    #[test]
    fn empty_params_are_noops() {
        let input = sample();
        let mut doc = parse(input);
        let before = emit(&doc);
        run_add_attributes_to_svg_element(&mut doc, &AddAttributesToSvgElementParams::default());
        run_add_classes_to_svg_element(&mut doc, &AddClassesToSvgElementParams::default());
        run_remove_attrs(&mut doc, &RemoveAttrsParams::default());
        run_remove_attributes_by_selector(&mut doc, &RemoveAttributesBySelectorParams::default());
        run_remove_elements_by_attr(&mut doc, &RemoveElementsByAttrParams::default());
        assert_eq!(emit(&doc), before);
    }

    #[test]
    fn default_pipeline_does_not_inject_or_honor_these() {
        let input = sample();
        let out = crate::optimize_str(input).unwrap();
        assert!(!out
            .plugins_run
            .iter()
            .any(|n| OPTIN_EDIT_PLUGIN_NAMES.contains(n)));
        assert!(
            out.svg.contains("data-k=\"1\"") || out.svg.contains("data-k='1'"),
            "{}",
            out.svg
        );
        assert!(
            !out.svg.contains("data-icon="),
            "addAttributes must not run by default: {}",
            out.svg
        );
    }

    #[test]
    fn add_attributes_writes_missing_keys_only_on_outer_svg() {
        let mut doc = parse(sample());
        run_add_attributes_to_svg_element(
            &mut doc,
            &AddAttributesToSvgElementParams::from_pairs([
                ("data-icon", "star"),
                ("role", "presentation"),
                ("focusable", "false"),
            ]),
        );
        let out = emit(&doc);
        assert!(out.contains("data-icon=\"star\""), "{out}");
        assert!(out.contains("role=\"img\""), "must not overwrite: {out}");
        assert!(out.contains("focusable=\"false\""), "{out}");
        let nested = out.find("class=\"nested\"").expect(out.as_str());
        assert!(
            !out[nested..].contains("data-icon"),
            "nested svg must stay untouched: {out}"
        );
    }

    #[test]
    fn add_classes_appends_without_duplicates() {
        let mut doc = parse(sample());
        run_add_classes_to_svg_element(
            &mut doc,
            &AddClassesToSvgElementParams::from_class_names(["root", "icon", "sprite"]),
        );
        let out = emit(&doc);
        assert!(out.contains("class=\"root icon sprite\""), "{out}");
        assert!(out.contains("class=\"nested\""), "{out}");

        let mut blank = parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><g/></svg>"#);
        run_add_classes_to_svg_element(
            &mut blank,
            &AddClassesToSvgElementParams::from_class_name("mark"),
        );
        let bout = emit(&blank);
        assert!(bout.contains("class=\"mark\""), "{bout}");
    }

    #[test]
    fn add_classes_class_names_overrides_class_name() {
        let mut doc = parse(r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#);
        let params = AddClassesToSvgElementParams {
            class_name: Some("ignored".into()),
            class_names: vec!["keep".into()],
        };
        run_add_classes_to_svg_element(&mut doc, &params);
        let out = emit(&doc);
        assert!(out.contains("class=\"keep\""), "{out}");
        assert!(!out.contains("ignored"), "{out}");
    }

    #[test]
    fn remove_attrs_globs_attr_elem_and_value() {
        let mut doc = parse(sample());
        run_remove_attrs(
            &mut doc,
            &RemoveAttrsParams::from_attrs(["fill", "stroke-*"]),
        );
        let out = emit(&doc);
        assert!(!out.contains("fill="), "{out}");
        assert!(!out.contains("stroke-width"), "{out}");
        assert!(out.contains("stroke=\"currentColor\""), "{out}");
        assert!(out.contains("data-k=\"1\""), "{out}");

        let mut scoped = parse(sample());
        run_remove_attrs(&mut scoped, &RemoveAttrsParams::from_attrs(["path:fill"]));
        let sout = emit(&scoped);
        assert!(sout.contains("fill=\"red\""), "{sout}");
        assert!(!sout.contains("fill=\"none\""), "{sout}");

        let mut valued = parse(sample());
        run_remove_attrs(&mut valued, &RemoveAttrsParams::from_attrs(["*:fill:none"]));
        let vout = emit(&valued);
        assert!(vout.contains("fill=\"red\""), "{vout}");
        assert!(!vout.contains("fill=\"none\""), "{vout}");
    }

    #[test]
    fn remove_attrs_preserves_current_color_and_xlink() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"##,
            r##"<use xlink:href="#box" fill="currentColor" stroke="red"/>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        let mut params = RemoveAttrsParams::from_attrs(["fill", "stroke", "xlink:href"]);
        params.preserve_current_color = true;
        run_remove_attrs(&mut doc, &params);
        let out = emit(&doc);
        assert!(out.contains("fill=\"currentColor\""), "{out}");
        assert!(!out.contains("stroke="), "{out}");
        assert!(!out.contains("xlink:href"), "{out}");
    }

    #[test]
    fn remove_attrs_custom_separator_and_data_glob() {
        let mut doc = parse(sample());
        let mut params = RemoveAttrsParams::from_attrs(["*|data-*"]);
        params.elem_separator = "|".into();
        run_remove_attrs(&mut doc, &params);
        let out = emit(&doc);
        assert!(!out.contains("data-k"), "{out}");
        assert!(out.contains("fill=\"red\""), "{out}");
    }

    #[test]
    fn remove_attributes_by_simple_selectors() {
        let mut by_id = parse(sample());
        run_remove_attributes_by_selector(
            &mut by_id,
            &RemoveAttributesBySelectorParams::single("#box", ["fill"]),
        );
        let iout = emit(&by_id);
        assert!(iout.contains("id=\"box\""), "{iout}");
        assert!(!iout.contains("fill=\"red\""), "{iout}");
        assert!(iout.contains("fill=\"none\""), "{iout}");

        let mut by_class = parse(sample());
        run_remove_attributes_by_selector(
            &mut by_class,
            &RemoveAttributesBySelectorParams::single(".icon", ["stroke-width"]),
        );
        let cout = emit(&by_class);
        assert!(!cout.contains("stroke-width"), "{cout}");

        let mut by_tag = parse(sample());
        run_remove_attributes_by_selector(
            &mut by_tag,
            &RemoveAttributesBySelectorParams::single("path", ["stroke"]),
        );
        let tout = emit(&by_tag);
        assert!(!tout.contains("stroke=\"currentColor\""), "{tout}");
        assert!(tout.contains("fill=\"red\""), "{tout}");

        let mut combo = parse(sample());
        run_remove_attributes_by_selector(
            &mut combo,
            &RemoveAttributesBySelectorParams::single("rect.icon", ["class"]),
        );
        let xout = emit(&combo);
        assert!(!xout.contains("class=\"icon x\""), "{xout}");
        assert!(xout.contains("class=\"root\""), "{xout}");
    }

    #[test]
    fn remove_attributes_selectors_list_and_refuse_complex() {
        let mut doc = parse(sample());
        run_remove_attributes_by_selector(
            &mut doc,
            &RemoveAttributesBySelectorParams {
                selectors: vec![
                    SelectorAttrRule::new("circle, path", ["fill"]),
                    SelectorAttrRule::new("#box", ["data-k"]),
                ],
                ..Default::default()
            },
        );
        let out = emit(&doc);
        assert!(!out.contains("fill=\"none\""), "{out}");
        assert!(!out.contains("data-k"), "{out}");
        assert!(out.contains("fill=\"red\""), "{out}");

        let mut complex = parse(sample());
        let before = emit(&complex);
        run_remove_attributes_by_selector(
            &mut complex,
            &RemoveAttributesBySelectorParams::single("g rect", ["fill"]),
        );
        run_remove_attributes_by_selector(
            &mut complex,
            &RemoveAttributesBySelectorParams::single("[fill='red']", ["fill"]),
        );
        assert_eq!(emit(&complex), before, "complex selectors must fail closed");
    }

    #[test]
    fn remove_elements_by_id_class_and_data() {
        let mut by_id = parse(sample());
        run_remove_elements_by_attr(&mut by_id, &RemoveElementsByAttrParams::from_ids(["box"]));
        let iout = emit(&by_id);
        assert!(!iout.contains("id=\"box\""), "{iout}");
        assert!(iout.contains("id=\"line\""), "{iout}");
        assert!(iout.starts_with("<svg") || iout.contains("<svg"), "{iout}");

        let mut by_class = parse(sample());
        run_remove_elements_by_attr(
            &mut by_class,
            &RemoveElementsByAttrParams::from_classes(["icon"]),
        );
        let cout = emit(&by_class);
        assert!(!cout.contains("id=\"box\""), "{cout}");
        assert!(cout.contains("id=\"line\""), "{cout}");

        let mut by_data = parse(sample());
        run_remove_elements_by_attr(
            &mut by_data,
            &RemoveElementsByAttrParams::from_data(["k=1"]),
        );
        let dout = emit(&by_data);
        assert!(!dout.contains("data-k"), "{dout}");
        assert!(dout.contains("<path"), "{dout}");

        let mut any_data = parse(sample());
        run_remove_elements_by_attr(
            &mut any_data,
            &RemoveElementsByAttrParams::from_data(["data-*"]),
        );
        let aout = emit(&any_data);
        assert!(!aout.contains("id=\"box\""), "{aout}");
    }

    #[test]
    fn remove_elements_never_drops_root_svg() {
        let mut doc = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg" id="root" class="x" data-z="1"><g id="g"/></svg>"#,
        );
        run_remove_elements_by_attr(
            &mut doc,
            &RemoveElementsByAttrParams {
                id: vec!["root".into()],
                class: vec!["x".into()],
                data: vec!["z".into()],
            },
        );
        let out = emit(&doc);
        assert!(out.contains("<svg"), "{out}");
        assert!(out.contains("id=\"root\""), "{out}");
        assert!(out.contains("<g"), "{out}");
    }

    #[test]
    fn add_attributes_skips_empty_names() {
        let mut doc = parse(r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#);
        run_add_attributes_to_svg_element(
            &mut doc,
            &AddAttributesToSvgElementParams::from_pairs([("", "x"), ("ok", "1")]),
        );
        let out = emit(&doc);
        assert!(out.contains("ok=\"1\""), "{out}");
        assert!(!out.contains("=\"x\""), "{out}");
    }

    #[test]
    fn json_params_and_enabled_pass() {
        let attrs = AddAttributesToSvgElementParams::from_value(&serde_json::json!({
            "attributes": { "data-icon": "star", "focusable": false }
        }));
        assert_eq!(
            attrs.attributes,
            vec![
                ("data-icon".into(), "star".into()),
                ("focusable".into(), "false".into())
            ]
        );

        let mixed = AddAttributesToSvgElementParams::from_value(&serde_json::json!({
            "attributes": ["mySvg", { "data-icon": "star" }, { "focusable": false }]
        }));
        assert_eq!(
            mixed.attributes,
            vec![
                ("mySvg".into(), "".into()),
                ("data-icon".into(), "star".into()),
                ("focusable".into(), "false".into())
            ]
        );
        let singular = AddAttributesToSvgElementParams::from_value(&serde_json::json!({
            "attribute": "size-big"
        }));
        assert_eq!(singular.attributes, vec![("size-big".into(), "".into())]);
        let shorthand = AddAttributesToSvgElementParams::from_value(&serde_json::json!({
            "data-n": 1
        }));
        assert_eq!(shorthand.attributes, vec![("data-n".into(), "1".into())]);

        let classes = AddClassesToSvgElementParams::from_value(&serde_json::json!({
            "className": "ignored",
            "classNames": ["a", "b"]
        }));
        assert_eq!(classes.class_names, vec!["a", "b"]);
        let class_list = AddClassesToSvgElementParams::from_value(&serde_json::json!({
            "className": ["keep", "me"]
        }));
        assert_eq!(class_list.class_names, vec!["keep", "me"]);
        let bare = AddClassesToSvgElementParams::from_value(&serde_json::json!(["x", "y"]));
        assert_eq!(bare.class_names, vec!["x", "y"]);

        let remove = RemoveAttrsParams::from_value(&serde_json::json!({
            "attrs": ["fill", "stroke-*"],
            "elemSeparator": "|",
            "preserveCurrentColor": true
        }));
        assert_eq!(remove.attrs, vec!["fill", "stroke-*"]);
        assert_eq!(remove.elem_separator, "|");
        assert!(remove.preserve_current_color);

        let by_sel = RemoveAttributesBySelectorParams::from_value(&serde_json::json!({
            "selector": "#box",
            "attributes": "fill"
        }));
        assert_eq!(by_sel.selector.as_deref(), Some("#box"));
        assert_eq!(by_sel.attributes, vec!["fill"]);

        let by_el = RemoveElementsByAttrParams::from_value(&serde_json::json!({
            "id": "box",
            "class": ["icon"],
            "data": "k=1"
        }));
        assert_eq!(by_el.id, vec!["box"]);
        assert_eq!(by_el.class, vec!["icon"]);
        assert_eq!(by_el.data, vec!["k=1"]);

        let mut cfg = crate::Config::default();
        assert!(!run_enabled_edit_pass(&mut parse(sample()), &cfg));

        cfg.enable_plugins.push("addAttributesToSVGElement".into());
        cfg.plugin_params.insert(
            "addAttributesToSVGElement".into(),
            serde_json::json!({ "attributes": { "data-icon": "star" } }),
        );
        cfg.enable_plugins.push("removeElementsByAttr".into());
        cfg.plugin_params.insert(
            "removeElementsByAttr".into(),
            serde_json::json!({ "id": "box" }),
        );
        let mut doc = parse(sample());
        assert!(run_enabled_edit_pass(&mut doc, &cfg));
        let out = emit(&doc);
        assert!(out.contains("data-icon=\"star\""), "{out}");
        assert!(!out.contains("id=\"box\""), "{out}");
    }

    #[test]
    fn plugin_params_wire_add_remove_through_optimize() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" class="root">"##,
            r##"<rect fill="red" stroke="currentColor" width="10" height="10"/>"##,
            "</svg>",
        );
        let default = crate::optimize_str(input).unwrap();
        assert!(
            !default
                .plugins_run
                .iter()
                .any(|n| OPTIN_EDIT_PLUGIN_NAMES.contains(n)),
            "{:?}",
            default.plugins_run
        );
        assert!(
            !default.svg.contains("data-icon="),
            "addAttributes stays off: {}",
            default.svg
        );

        let mut cfg = crate::Config::default();
        cfg.enable_plugins.push("addAttributesToSVGElement".into());
        cfg.enable_plugins.push("addClassesToSVGElement".into());
        cfg.enable_plugins.push("removeAttrs".into());
        cfg.plugin_params.insert(
            "addAttributesToSVGElement".into(),
            serde_json::json!({
                "attributes": [{ "data-icon": "star" }, "focusable"]
            }),
        );
        cfg.plugin_params.insert(
            "addClassesToSVGElement".into(),
            serde_json::json!({ "className": "mark", "classNames": ["sprite"] }),
        );
        cfg.plugin_params.insert(
            "removeAttrs".into(),
            serde_json::json!({
                "attrs": ["fill", "stroke"],
                "preserveCurrentColor": true
            }),
        );
        let out = crate::optimize_str_with(input, &cfg).unwrap();
        assert!(out.svg.contains("data-icon=\"star\""), "{}", out.svg);
        assert!(out.svg.contains("focusable=\"\""), "{}", out.svg);
        assert!(out.svg.contains("class=\"root sprite\""), "{}", out.svg);
        assert!(!out.svg.contains("fill="), "{}", out.svg);
        assert!(
            out.svg.contains("stroke=\"currentColor\"")
                || out.svg.contains("stroke=\"currentcolor\""),
            "{}",
            out.svg
        );
        assert!(!out.svg.contains("class=\"mark\""), "{}", out.svg);
    }
}
