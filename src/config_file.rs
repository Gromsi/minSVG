//! Load minsvg / SVGO-shaped config files.
//!
//! Formats: TOML, JSON, and a small YAML subset (`*.yml` / `*.yaml`) — no
//! `serde_yaml` (keeps the binary lean). Keys: `animation_aware`, `skip`,
//! `precision` / `floatPrecision`, `multipass`, `plugins` (`name` / `active` /
//! `params`), `js2svg.pretty` / `indent`.
//!
//! CLI flags override file values when the user passes them.
//! [`load_config_path`] applies a file onto [`Config::default`] (no CLI).

use crate::Config;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_TOML: &str = "minsvg.config.toml";
pub const CONFIG_JSON: &str = "minsvg.config.json";
pub const CONFIG_YAML: &str = "minsvg.config.yaml";
pub const CONFIG_YML: &str = "minsvg.config.yml";
pub const SVGO_CONFIG_JSON: &str = "svgo.config.json";
pub const SVGO_CONFIG_YAML: &str = "svgo.config.yaml";
pub const SVGO_CONFIG_YML: &str = "svgo.config.yml";

/// Auto-discover order in a directory (first existing file wins).
const DIR_CONFIG_NAMES: &[&str] = &[
    CONFIG_TOML,
    CONFIG_JSON,
    CONFIG_YAML,
    CONFIG_YML,
    SVGO_CONFIG_JSON,
    SVGO_CONFIG_YAML,
    SVGO_CONFIG_YML,
];

#[derive(Debug, thiserror::Error)]
pub enum ConfigFileError {
    #[error("read {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse {}: {message}", .path.display())]
    Parse { path: PathBuf, message: String },
}

/// Optional values from a minsvg / SVGO-shaped config file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    #[serde(alias = "animation-aware")]
    pub animation_aware: Option<bool>,
    pub skip: Option<Vec<String>>,
    #[serde(alias = "floatPrecision")]
    pub precision: Option<u8>,
    pub multipass: Option<bool>,
    /// Top-level pretty (non-SVGO). [`Self::js2svg`] wins when both are set.
    pub pretty: Option<bool>,
    pub indent: Option<u8>,
    pub js2svg: Option<Js2SvgConfig>,
    pub plugins: Option<Vec<FilePlugin>>,
}

/// SVGO `js2svg` emit knobs we honor (`pretty` / `indent`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Js2SvgConfig {
    pub pretty: Option<bool>,
    pub indent: Option<u8>,
}

/// SVGO plugin list entry: a name string or `{ name, active, params }`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum FilePlugin {
    Name(String),
    Spec {
        name: String,
        #[serde(default)]
        active: Option<bool>,
        #[serde(default)]
        params: Option<Value>,
    },
}

impl FileConfig {
    pub fn apply(&self, cfg: &mut Config) {
        if let Some(v) = self.animation_aware {
            cfg.animation_aware = v;
        }
        if let Some(ref skip) = self.skip {
            cfg.skip_plugins.clone_from(skip);
        }
        if let Some(p) = self.precision {
            cfg.precision = Some(p);
        }
        if let Some(m) = self.multipass {
            cfg.multipass = m;
        }
        if let Some(p) = self.pretty {
            cfg.pretty = p;
        }
        if let Some(i) = self.indent {
            cfg.indent = i;
        }
        if let Some(ref js2svg) = self.js2svg {
            if let Some(p) = js2svg.pretty {
                cfg.pretty = p;
            }
            if let Some(i) = js2svg.indent {
                cfg.indent = i;
            }
        }
        if let Some(ref plugins) = self.plugins {
            apply_file_plugins(cfg, plugins);
        }
    }
}

/// Flags that override a loaded file. Unset / empty means “keep file or default”.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliOverrides {
    pub no_animation_aware: bool,
    pub skip: Vec<String>,
    /// Enable named passes (unskip). Applied before `--skip` so skip wins ties.
    pub plugin: Vec<String>,
    pub precision: Option<u8>,
    pub multipass: bool,
}

impl CliOverrides {
    pub fn apply(&self, cfg: &mut Config) {
        if self.no_animation_aware {
            cfg.animation_aware = false;
        }
        for name in &self.plugin {
            cfg.skip_plugins.retain(|s| !s.eq_ignore_ascii_case(name));
            if !cfg
                .enable_plugins
                .iter()
                .any(|s| s.eq_ignore_ascii_case(name))
            {
                cfg.enable_plugins.push(name.clone());
            }
        }
        for name in &self.skip {
            cfg.enable_plugins.retain(|s| !s.eq_ignore_ascii_case(name));
            if !cfg
                .skip_plugins
                .iter()
                .any(|s| s.eq_ignore_ascii_case(name))
            {
                cfg.skip_plugins.push(name.clone());
            }
        }
        if let Some(p) = self.precision {
            cfg.precision = Some(p);
        }
        if self.multipass {
            cfg.multipass = true;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub config: FileConfig,
}

pub fn parse_config_toml(text: &str) -> Result<FileConfig, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

pub fn parse_config_json(text: &str) -> Result<FileConfig, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

pub fn parse_config_yaml(text: &str) -> Result<FileConfig, String> {
    let value = yaml_subset::to_value(text)?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Load JSON / YAML / TOML at `path` onto [`Config::default`].
pub fn load_config_path(path: &Path) -> Result<Config, ConfigFileError> {
    let loaded = load_config_from_path(path)?;
    Ok(merge_config(Some(&loaded.config), &CliOverrides::default()))
}

pub fn merge_config(file: Option<&FileConfig>, cli: &CliOverrides) -> Config {
    let mut cfg = Config::default();
    if let Some(file) = file {
        file.apply(&mut cfg);
    }
    cli.apply(&mut cfg);
    cfg
}

pub fn load_config_from_cwd() -> Result<Option<LoadedConfig>, ConfigFileError> {
    let dir = std::env::current_dir().map_err(|e| ConfigFileError::Io {
        path: PathBuf::from("."),
        source: e,
    })?;
    load_config_from_dir(&dir)
}

/// Prefer `minsvg.config.toml` when several config files exist.
pub fn load_config_from_dir(dir: &Path) -> Result<Option<LoadedConfig>, ConfigFileError> {
    for name in DIR_CONFIG_NAMES {
        let path = dir.join(name);
        if path.is_file() {
            return load_config_from_path(&path).map(Some);
        }
    }
    Ok(None)
}

pub fn load_config_from_path(path: &Path) -> Result<LoadedConfig, ConfigFileError> {
    let text = fs::read_to_string(path).map_err(|e| ConfigFileError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let config = parse_by_ext(path, &text).map_err(|message| ConfigFileError::Parse {
        path: path.to_path_buf(),
        message,
    })?;
    Ok(LoadedConfig {
        path: path.to_path_buf(),
        config,
    })
}

pub fn config_from_cwd_and_cli(cli: &CliOverrides) -> Result<Config, ConfigFileError> {
    let loaded = load_config_from_cwd()?;
    Ok(merge_config(loaded.as_ref().map(|l| &l.config), cli))
}

pub fn config_from_path_and_cli(
    path: &Path,
    cli: &CliOverrides,
) -> Result<Config, ConfigFileError> {
    let loaded = load_config_from_path(path)?;
    Ok(merge_config(Some(&loaded.config), cli))
}

pub fn config_from_dir_and_cli(dir: &Path, cli: &CliOverrides) -> Result<Config, ConfigFileError> {
    let loaded = load_config_from_dir(dir)?;
    Ok(merge_config(loaded.as_ref().map(|l| &l.config), cli))
}

fn parse_by_ext(path: &Path, text: &str) -> Result<FileConfig, String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("json") => parse_config_json(text),
        Some(ext) if ext.eq_ignore_ascii_case("toml") => parse_config_toml(text),
        Some(ext) if ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml") => {
            parse_config_yaml(text)
        }
        _ => parse_config_toml(text)
            .or_else(|_| parse_config_json(text))
            .or_else(|_| parse_config_yaml(text)),
    }
}

fn apply_file_plugins(cfg: &mut Config, plugins: &[FilePlugin]) {
    for item in plugins {
        match item {
            FilePlugin::Name(name) => {
                if name.eq_ignore_ascii_case("preset-default") {
                    continue;
                }
                enable_named(cfg, name, None);
            }
            FilePlugin::Spec {
                name,
                active,
                params,
            } => {
                if name.eq_ignore_ascii_case("preset-default") {
                    if let Some(overrides) = params.as_ref().and_then(|v| v.get("overrides")) {
                        apply_preset_overrides(cfg, overrides);
                    }
                    continue;
                }
                if *active == Some(false) {
                    skip_named(cfg, name);
                } else {
                    enable_named(cfg, name, params.as_ref());
                }
            }
        }
    }
}

fn apply_preset_overrides(cfg: &mut Config, overrides: &Value) {
    let Some(map) = overrides.as_object() else {
        return;
    };
    for (name, val) in map {
        match val {
            Value::Bool(false) => skip_named(cfg, name),
            Value::Bool(true) => enable_named(cfg, name, None),
            Value::Object(obj) => {
                if obj.get("active").and_then(|v| v.as_bool()) == Some(false) {
                    skip_named(cfg, name);
                    continue;
                }
                let mut rest = obj.clone();
                rest.remove("active");
                let params = if rest.is_empty() {
                    None
                } else {
                    Some(Value::Object(rest))
                };
                enable_named(cfg, name, params.as_ref());
            }
            _ => {}
        }
    }
}

fn skip_named(cfg: &mut Config, name: &str) {
    cfg.enable_plugins.retain(|s| !s.eq_ignore_ascii_case(name));
    remove_plugin_param(cfg, name);
    if !cfg
        .skip_plugins
        .iter()
        .any(|s| s.eq_ignore_ascii_case(name))
    {
        cfg.skip_plugins.push(name.to_string());
    }
}

fn enable_named(cfg: &mut Config, name: &str, params: Option<&Value>) {
    cfg.skip_plugins.retain(|s| !s.eq_ignore_ascii_case(name));
    if !cfg
        .enable_plugins
        .iter()
        .any(|s| s.eq_ignore_ascii_case(name))
    {
        cfg.enable_plugins.push(name.to_string());
    }
    remove_plugin_param(cfg, name);
    if let Some(value) = params.filter(|v| match v {
        Value::Object(m) => !m.is_empty(),
        _ => false,
    }) {
        insert_plugin_param(cfg, name, value.clone());
    }
}

fn remove_plugin_param(cfg: &mut Config, name: &str) {
    if let Some(existing) = cfg
        .plugin_params
        .keys()
        .find(|k| k.eq_ignore_ascii_case(name))
        .cloned()
    {
        cfg.plugin_params.remove(&existing);
    }
}

fn insert_plugin_param(cfg: &mut Config, name: &str, value: Value) {
    if let Some(existing) = cfg
        .plugin_params
        .keys()
        .find(|k| k.eq_ignore_ascii_case(name))
        .cloned()
    {
        cfg.plugin_params.insert(existing, value);
    } else {
        cfg.plugin_params.insert(name.to_string(), value);
    }
}

/// Indent-based YAML subset for the keys we actually read. No anchors, tags,
/// or flow collections — convert to JSON [`Value`] then reuse serde.
mod yaml_subset {
    use serde_json::{Map, Number, Value};

    struct Line {
        no: usize,
        indent: usize,
        text: String,
    }

    pub fn to_value(text: &str) -> Result<Value, String> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let lines = preprocess(text)?;
        if lines.is_empty() {
            return Ok(Value::Object(Map::new()));
        }
        let (value, next) = parse_node(&lines, 0)?;
        if next != lines.len() {
            return Err(format!("line {}: unexpected indentation", lines[next].no));
        }
        if !value.is_object() {
            return Err("config root must be a mapping".into());
        }
        Ok(value)
    }

    fn preprocess(text: &str) -> Result<Vec<Line>, String> {
        let mut out = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let no = i + 1;
            let expanded = raw.replace('\t', "  ");
            let stripped = strip_comment(&expanded);
            if stripped.chars().all(|c| c == ' ') {
                continue;
            }
            let trimmed = stripped.trim();
            if trimmed == "---" || trimmed == "..." {
                continue;
            }
            let indent = stripped.len() - stripped.trim_start_matches(' ').len();
            out.push(Line {
                no,
                indent,
                text: stripped.trim().to_string(),
            });
        }
        Ok(out)
    }

    fn strip_comment(s: &str) -> &str {
        let mut in_single = false;
        let mut in_double = false;
        for (i, c) in s.char_indices() {
            match c {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                '#' if !in_single && !in_double => return s[..i].trim_end(),
                _ => {}
            }
        }
        s.trim_end()
    }

    fn parse_node(lines: &[Line], i: usize) -> Result<(Value, usize), String> {
        if i >= lines.len() {
            return Ok((Value::Null, i));
        }
        if is_seq_item(&lines[i].text) {
            parse_seq(lines, i, lines[i].indent)
        } else {
            parse_map(lines, i, lines[i].indent)
        }
    }

    fn parse_map(
        lines: &[Line],
        mut i: usize,
        key_indent: usize,
    ) -> Result<(Value, usize), String> {
        let mut map = Map::new();
        while i < lines.len() && lines[i].indent == key_indent {
            if is_seq_item(&lines[i].text) {
                break;
            }
            let (key, rest) = split_kv(&lines[i].text)
                .ok_or_else(|| format!("line {}: expected `key: value`", lines[i].no))?;
            i += 1;
            let value = if !rest.is_empty() {
                parse_scalar(&rest)
            } else if i < lines.len() && lines[i].indent > key_indent {
                let (nested, ni) = parse_node(lines, i)?;
                i = ni;
                nested
            } else {
                Value::Null
            };
            map.insert(key, value);
        }
        Ok((Value::Object(map), i))
    }

    fn parse_seq(
        lines: &[Line],
        mut i: usize,
        dash_indent: usize,
    ) -> Result<(Value, usize), String> {
        let mut arr = Vec::new();
        while i < lines.len() && lines[i].indent == dash_indent && is_seq_item(&lines[i].text) {
            let rest = strip_dash(&lines[i].text);
            i += 1;
            if rest.is_empty() {
                if i < lines.len() && lines[i].indent > dash_indent {
                    let (nested, ni) = parse_node(lines, i)?;
                    i = ni;
                    arr.push(nested);
                } else {
                    arr.push(Value::Null);
                }
            } else if let Some((key, valrest)) = split_kv(rest) {
                let mut obj = Map::new();
                if !valrest.is_empty() {
                    obj.insert(key, parse_scalar(&valrest));
                } else if i < lines.len() && lines[i].indent > dash_indent {
                    let (nested, ni) = parse_node(lines, i)?;
                    i = ni;
                    obj.insert(key, nested);
                } else {
                    obj.insert(key, Value::Null);
                }
                if i < lines.len() && lines[i].indent > dash_indent && !is_seq_item(&lines[i].text)
                {
                    let (more, ni) = parse_map(lines, i, lines[i].indent)?;
                    i = ni;
                    if let Value::Object(extra) = more {
                        for (k, v) in extra {
                            obj.insert(k, v);
                        }
                    }
                }
                arr.push(Value::Object(obj));
            } else {
                arr.push(parse_scalar(rest));
            }
        }
        Ok((Value::Array(arr), i))
    }

    fn is_seq_item(text: &str) -> bool {
        text == "-" || text.starts_with("- ")
    }

    fn strip_dash(text: &str) -> &str {
        text.strip_prefix("- ")
            .or_else(|| text.strip_prefix('-'))
            .unwrap_or(text)
            .trim()
    }

    fn split_kv(text: &str) -> Option<(String, String)> {
        let mut in_single = false;
        let mut in_double = false;
        for (i, c) in text.char_indices() {
            match c {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                ':' if !in_single && !in_double => {
                    let key = text[..i].trim();
                    if key.is_empty() {
                        return None;
                    }
                    let rest = text[i + 1..].trim();
                    return Some((unquote_or_keep(key), rest.to_string()));
                }
                _ => {}
            }
        }
        None
    }

    fn parse_scalar(s: &str) -> Value {
        let s = s.trim();
        if s.is_empty() || s == "~" || s.eq_ignore_ascii_case("null") {
            return Value::Null;
        }
        if s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes") {
            return Value::Bool(true);
        }
        if s.eq_ignore_ascii_case("false") || s.eq_ignore_ascii_case("no") {
            return Value::Bool(false);
        }
        if let Some(inner) = unquote(s) {
            return Value::String(inner);
        }
        if let Ok(n) = s.parse::<u64>() {
            return Value::Number(n.into());
        }
        if let Ok(n) = s.parse::<i64>() {
            return Value::Number(n.into());
        }
        if let Ok(f) = s.parse::<f64>() {
            if f.fract() == 0.0 && f >= 0.0 && f <= u64::MAX as f64 {
                return Value::Number((f as u64).into());
            }
            if let Some(num) = Number::from_f64(f) {
                return Value::Number(num);
            }
        }
        Value::String(s.to_string())
    }

    fn unquote_or_keep(s: &str) -> String {
        unquote(s).unwrap_or_else(|| s.to_string())
    }

    fn unquote(s: &str) -> Option<String> {
        if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            return Some(unescape_double(&s[1..s.len() - 1]));
        }
        if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
            return Some(s[1..s.len() - 1].replace("''", "'"));
        }
        None
    }

    fn unescape_double(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some(other) => out.push(other),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_cfg(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn loads_toml_from_tempfile_dir() {
        let dir = tempfile::tempdir().unwrap();
        write_cfg(
            dir.path(),
            CONFIG_TOML,
            r#"
animation_aware = false
skip = ["cleanupIds", "mergePaths"]
precision = 2
multipass = true
"#,
        );
        let loaded = load_config_from_dir(dir.path()).unwrap().unwrap();
        assert!(
            loaded.path.ends_with(CONFIG_TOML),
            "{}",
            loaded.path.display()
        );
        assert_eq!(loaded.config.animation_aware, Some(false));
        assert_eq!(
            loaded.config.skip,
            Some(vec!["cleanupIds".into(), "mergePaths".into()])
        );
        assert_eq!(loaded.config.precision, Some(2));
        assert_eq!(loaded.config.multipass, Some(true));
    }

    #[test]
    fn loads_json_from_tempfile_dir() {
        let dir = tempfile::tempdir().unwrap();
        write_cfg(
            dir.path(),
            CONFIG_JSON,
            r#"{"animation_aware":false,"skip":["cleanupIds"],"precision":3,"multipass":true}"#,
        );
        let loaded = load_config_from_dir(dir.path()).unwrap().unwrap();
        assert!(
            loaded.path.ends_with(CONFIG_JSON),
            "{}",
            loaded.path.display()
        );
        assert_eq!(loaded.config.animation_aware, Some(false));
        assert_eq!(loaded.config.skip, Some(vec!["cleanupIds".into()]));
        assert_eq!(loaded.config.precision, Some(3));
        assert_eq!(loaded.config.multipass, Some(true));
    }

    #[test]
    fn prefers_toml_when_both_exist() {
        let dir = tempfile::tempdir().unwrap();
        write_cfg(dir.path(), CONFIG_TOML, "precision = 2\n");
        write_cfg(dir.path(), CONFIG_JSON, r#"{"precision":3}"#);
        let loaded = load_config_from_dir(dir.path()).unwrap().unwrap();
        assert!(loaded.path.ends_with(CONFIG_TOML));
        assert_eq!(loaded.config.precision, Some(2));
    }

    #[test]
    fn missing_config_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_config_from_dir(dir.path()).unwrap().is_none());
    }

    #[test]
    fn invalid_toml_errors() {
        let dir = tempfile::tempdir().unwrap();
        write_cfg(dir.path(), CONFIG_TOML, "precision = [\n");
        let err = load_config_from_dir(dir.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("parse"), "{msg}");
        assert!(msg.contains(CONFIG_TOML), "{msg}");
    }

    #[test]
    fn cli_overrides_file_values() {
        let file = FileConfig {
            animation_aware: Some(true),
            skip: Some(vec!["cleanupIds".into()]),
            precision: Some(3),
            multipass: Some(false),
            ..FileConfig::default()
        };
        let cfg = merge_config(
            Some(&file),
            &CliOverrides {
                no_animation_aware: true,
                skip: vec!["mergePaths".into()],
                plugin: vec![],
                precision: Some(2),
                multipass: true,
            },
        );
        assert!(!cfg.animation_aware);
        assert_eq!(cfg.precision, Some(2));
        assert!(cfg.multipass);
        assert!(cfg.skip_plugins.iter().any(|s| s == "cleanupIds"));
        assert!(cfg.skip_plugins.iter().any(|s| s == "mergePaths"));
    }

    #[test]
    fn file_used_when_cli_unset() {
        let file = FileConfig {
            animation_aware: Some(false),
            skip: Some(vec!["cleanupIds".into()]),
            precision: Some(2),
            multipass: Some(true),
            ..FileConfig::default()
        };
        let cfg = merge_config(Some(&file), &CliOverrides::default());
        assert!(!cfg.animation_aware);
        assert_eq!(cfg.skip_plugins, vec!["cleanupIds".to_string()]);
        assert_eq!(cfg.precision, Some(2));
        assert!(cfg.multipass);
    }

    #[test]
    fn config_from_tempfile_dir_then_cli_override() {
        let dir = tempfile::tempdir().unwrap();
        write_cfg(
            dir.path(),
            CONFIG_TOML,
            "animation_aware = false\nprecision = 3\nmultipass = true\nskip = [\"cleanupIds\"]\n",
        );
        let cfg = config_from_dir_and_cli(
            dir.path(),
            &CliOverrides {
                no_animation_aware: true,
                skip: vec!["mergePaths".into()],
                plugin: vec![],
                precision: Some(2),
                multipass: false,
            },
        )
        .unwrap();
        assert!(!cfg.animation_aware);
        assert_eq!(cfg.precision, Some(2));
        assert!(
            cfg.multipass,
            "unset --multipass must keep file multipass = true"
        );
        assert_eq!(cfg.skip_plugins.len(), 2);
    }

    #[test]
    fn parse_json_float_precision_alias() {
        let file = parse_config_json(r#"{"floatPrecision":2}"#).unwrap();
        assert_eq!(file.precision, Some(2));
    }

    #[test]
    fn plugin_flag_unskips_file_skip() {
        let file = FileConfig {
            skip: Some(vec!["cleanupIds".into(), "mergePaths".into()]),
            ..FileConfig::default()
        };
        let cfg = merge_config(
            Some(&file),
            &CliOverrides {
                plugin: vec!["cleanupIds".into()],
                skip: vec!["convertPathData".into()],
                ..CliOverrides::default()
            },
        );
        assert!(
            !cfg.skip_plugins.iter().any(|s| s == "cleanupIds"),
            "{:?}",
            cfg.skip_plugins
        );
        assert!(cfg.enable_plugins.iter().any(|s| s == "cleanupIds"));
        assert!(cfg.skip_plugins.iter().any(|s| s == "mergePaths"));
        assert!(cfg.skip_plugins.iter().any(|s| s == "convertPathData"));
    }

    #[test]
    fn skip_wins_when_same_name_is_plugin_and_skip() {
        let cfg = merge_config(
            None,
            &CliOverrides {
                plugin: vec!["cleanupIds".into()],
                skip: vec!["cleanupIds".into()],
                ..CliOverrides::default()
            },
        );
        assert_eq!(cfg.skip_plugins, vec!["cleanupIds".to_string()]);
    }

    fn write_named(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn load_config_path_svgo_json_tempfile() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_named(
            dir.path(),
            "svgo.config.json",
            r#"{
              "multipass": true,
              "floatPrecision": 2,
              "js2svg": { "pretty": true, "indent": 2 },
              "plugins": [
                { "name": "removeViewBox", "active": false },
                { "name": "cleanupIds", "params": { "minify": false } },
                "cleanupListOfValues"
              ]
            }"#,
        );
        let cfg = load_config_path(&path).unwrap();
        assert!(cfg.multipass);
        assert_eq!(cfg.precision, Some(2));
        assert!(cfg.pretty);
        assert_eq!(cfg.indent, 2);
        assert!(cfg.skip_plugins.iter().any(|s| s == "removeViewBox"));
        assert!(cfg.enable_plugins.iter().any(|s| s == "cleanupIds"));
        assert!(cfg
            .enable_plugins
            .iter()
            .any(|s| s == "cleanupListOfValues"));
        assert_eq!(cfg.plugin_param("cleanupIds").unwrap()["minify"], false);
        assert!(!cfg.plugin_enabled("removeViewBox"));
        assert!(cfg.plugin_enabled("cleanupListOfValues"));
    }

    #[test]
    fn load_config_path_yaml_and_yml_tempfiles() {
        let yaml_body = r#"
# comment
multipass: true
floatPrecision: 3
js2svg:
  pretty: true
  indent: 2
plugins:
  - name: removeComments
    active: false
  - name: cleanupListOfValues
    params:
      floatPrecision: 2
  - prefixIds
"#;
        let dir = tempfile::tempdir().unwrap();
        let yaml_path = write_named(dir.path(), "svgo.config.yaml", yaml_body);
        let yml_path = write_named(dir.path(), "extra.yml", yaml_body);
        for path in [&yaml_path, &yml_path] {
            let cfg = load_config_path(path).unwrap();
            assert!(cfg.multipass, "{}", path.display());
            assert_eq!(cfg.precision, Some(3));
            assert!(cfg.pretty);
            assert_eq!(cfg.indent, 2);
            assert!(cfg.skip_plugins.iter().any(|s| s == "removeComments"));
            assert_eq!(
                cfg.plugin_param("cleanupListOfValues").unwrap()["floatPrecision"],
                2
            );
            assert!(cfg.plugin_enabled("cleanupListOfValues"));
            assert!(cfg.plugin_enabled("prefixIds"));
        }
    }

    #[test]
    fn yaml_preset_default_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_named(
            dir.path(),
            "svgo.config.yml",
            r#"
plugins:
  - name: preset-default
    params:
      overrides:
        convertPathData: false
        mergePaths:
          active: false
        cleanupIds: true
        cleanupListOfValues:
          floatPrecision: 2
"#,
        );
        let cfg = load_config_path(&path).unwrap();
        assert!(cfg.skip_plugins.iter().any(|s| s == "convertPathData"));
        assert!(cfg.skip_plugins.iter().any(|s| s == "mergePaths"));
        assert!(cfg.enable_plugins.iter().any(|s| s == "cleanupIds"));
        assert!(cfg
            .enable_plugins
            .iter()
            .any(|s| s == "cleanupListOfValues"));
        assert_eq!(
            cfg.plugin_param("cleanupListOfValues").unwrap()["floatPrecision"],
            2
        );
        assert!(!cfg.enable_plugins.iter().any(|s| s == "preset-default"));
    }

    #[test]
    fn plugin_last_entry_wins_and_active_false_drops_params() {
        let file = parse_config_json(
            r#"{
              "plugins": [
                { "name": "cleanupListOfValues", "params": { "floatPrecision": 1 } },
                { "name": "cleanupListOfValues", "active": false }
              ]
            }"#,
        )
        .unwrap();
        let cfg = merge_config(Some(&file), &CliOverrides::default());
        assert!(cfg.skip_plugins.iter().any(|s| s == "cleanupListOfValues"));
        assert!(cfg.plugin_param("cleanupListOfValues").is_none());
    }

    #[test]
    fn load_config_from_dir_finds_svgo_yaml() {
        let dir = tempfile::tempdir().unwrap();
        write_cfg(
            dir.path(),
            SVGO_CONFIG_YAML,
            "multipass: true\nprecision: 2\n",
        );
        let loaded = load_config_from_dir(dir.path()).unwrap().unwrap();
        assert!(loaded.path.ends_with(SVGO_CONFIG_YAML));
        assert_eq!(loaded.config.multipass, Some(true));
        assert_eq!(loaded.config.precision, Some(2));
    }

    #[test]
    fn prefers_minsvg_toml_over_svgo_yaml() {
        let dir = tempfile::tempdir().unwrap();
        write_cfg(dir.path(), CONFIG_TOML, "precision = 1\n");
        write_cfg(dir.path(), SVGO_CONFIG_YAML, "precision: 4\n");
        let loaded = load_config_from_dir(dir.path()).unwrap().unwrap();
        assert!(loaded.path.ends_with(CONFIG_TOML));
        assert_eq!(loaded.config.precision, Some(1));
    }

    #[test]
    fn invalid_yaml_errors() {
        let dir = tempfile::tempdir().unwrap();
        write_cfg(dir.path(), "broken.yaml", ":\n  - nope\n");
        let err = load_config_from_path(&dir.path().join("broken.yaml")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("parse"), "{msg}");
        assert!(msg.contains("broken.yaml"), "{msg}");
    }

    #[test]
    fn yaml_crlf_and_quoted_hash() {
        let file = parse_config_yaml("pretty: true\r\nskip:\r\n  - \"foo#bar\"\r\n").unwrap();
        assert_eq!(file.pretty, Some(true));
        assert_eq!(file.skip, Some(vec!["foo#bar".into()]));
    }
}
