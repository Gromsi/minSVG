//! Optimizer [`Config`] plus CLI plugin-param parsing.
//!
//! Task 10/12 JS mapping: `optimize(svg, { plugins: [{ name, params }] })`
//! becomes `--plugin name` plus `--param name=<json>` (or one token
//! `--plugin name:{"foo":1}`). Last `--param` wins over `--plugin` JSON.

use crate::serialize::DataUri;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Config {
    /// Reserved for future JSON/TOML presets. v1 always runs the MVP default set.
    pub preset: Preset,
    /// Skip motion-unsafe passes when SMIL/CSS/script/external IDs are detected.
    pub animation_aware: bool,
    /// Optional sibling JS/TS/CSS/JSX texts (name, contents) for `#id` refs.
    /// Full workspace directory scan is TODO — pass files via `--extra`.
    pub extra_sources: Vec<(String, String)>,
    /// Display name for the optimize summary line.
    pub source_name: Option<String>,
    /// Plugin names to skip (CLI `--skip`; JS `{ name, active: false }`).
    pub skip_plugins: Vec<String>,
    /// Re-run the plugin set until serialized size is stable (max 10).
    pub multipass: bool,
    /// Decimal places for path `d` and numeric attrs (`--precision`).
    ///
    /// `None` keeps lossless path emit and numeric default 3. Motion-sensitive
    /// docs still skip path rewrite. Dest-count / digit-glue / bbox guards stay.
    pub precision: Option<u8>,
    /// Pretty-print the serialized SVG (`--pretty`). Plugin loop stays compact.
    pub pretty: bool,
    /// Spaces per indent when `pretty` (SVGO `--indent`, default 4).
    pub indent: u8,
    /// Wrap the serialized SVG as a data URI (`--datauri` base64 / enc / unenc).
    pub datauri: Option<DataUri>,
    /// Per-plugin JSON params (`--param name=json` / `--plugin name:{…}`).
    pub plugin_params: HashMap<String, Value>,
    /// Named passes requested via `--plugin` (opt-in IDs + unskip names).
    pub enable_plugins: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            preset: Preset::Default,
            animation_aware: true,
            extra_sources: Vec::new(),
            source_name: None,
            skip_plugins: Vec::new(),
            multipass: false,
            precision: None,
            pretty: false,
            indent: 4,
            datauri: None,
            plugin_params: HashMap::new(),
            enable_plugins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Preset {
    #[default]
    Default,
}

/// `--plugin name` or `--plugin name:{"foo":1}`.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginSpec {
    pub name: String,
    pub params: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginParamError {
    #[error("invalid --plugin `{spec}`: {message}")]
    Plugin { spec: String, message: String },
    #[error("invalid --param `{spec}`: {message}")]
    Param { spec: String, message: String },
}

impl Config {
    pub fn plugin_param(&self, name: &str) -> Option<&Value> {
        self.plugin_params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }

    pub fn plugin_enabled(&self, name: &str) -> bool {
        if self
            .skip_plugins
            .iter()
            .any(|s| s.eq_ignore_ascii_case(name))
        {
            return false;
        }
        self.enable_plugins
            .iter()
            .any(|s| s.eq_ignore_ascii_case(name))
    }
}

/// Parse `--plugin name` or `--plugin name:<json>`.
pub fn parse_plugin_spec(spec: &str) -> Result<PluginSpec, PluginParamError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(PluginParamError::Plugin {
            spec: spec.to_string(),
            message: "empty plugin name".into(),
        });
    }
    match spec.split_once(':') {
        None => Ok(PluginSpec {
            name: spec.to_string(),
            params: None,
        }),
        Some((name, rest)) => {
            let name = name.trim();
            if name.is_empty() {
                return Err(PluginParamError::Plugin {
                    spec: spec.to_string(),
                    message: "empty plugin name".into(),
                });
            }
            let rest = rest.trim();
            if rest.is_empty() {
                return Ok(PluginSpec {
                    name: name.to_string(),
                    params: None,
                });
            }
            let params = serde_json::from_str(rest).map_err(|e| PluginParamError::Plugin {
                spec: spec.to_string(),
                message: e.to_string(),
            })?;
            Ok(PluginSpec {
                name: name.to_string(),
                params: Some(params),
            })
        }
    }
}

/// Parse `--param name=json`.
pub fn parse_param_spec(spec: &str) -> Result<(String, Value), PluginParamError> {
    let spec = spec.trim();
    let Some((name, json)) = spec.split_once('=') else {
        return Err(PluginParamError::Param {
            spec: spec.to_string(),
            message: "expected name=json".into(),
        });
    };
    let name = name.trim();
    if name.is_empty() {
        return Err(PluginParamError::Param {
            spec: spec.to_string(),
            message: "empty plugin name".into(),
        });
    }
    let value = serde_json::from_str(json.trim()).map_err(|e| PluginParamError::Param {
        spec: spec.to_string(),
        message: e.to_string(),
    })?;
    Ok((name.to_string(), value))
}

/// Apply `--plugin` specs then `--param` (param wins on the same name).
///
/// `--plugin` records the name in [`Config::enable_plugins`]. `--param`
/// only sets JSON (does not enable a default-off pass).
pub fn apply_plugin_cli(
    cfg: &mut Config,
    plugins: &[PluginSpec],
    params: &[String],
) -> Result<(), PluginParamError> {
    for spec in plugins {
        enable_plugin(cfg, &spec.name);
        if let Some(ref value) = spec.params {
            insert_plugin_param(cfg, &spec.name, value.clone());
        }
    }
    for raw in params {
        let (name, value) = parse_param_spec(raw)?;
        insert_plugin_param(cfg, &name, value);
    }
    Ok(())
}

fn enable_plugin(cfg: &mut Config, name: &str) {
    if !cfg
        .enable_plugins
        .iter()
        .any(|s| s.eq_ignore_ascii_case(name))
    {
        cfg.enable_plugins.push(name.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_empty_plugin_params() {
        let cfg = Config::default();
        assert!(cfg.plugin_params.is_empty());
        assert!(cfg.enable_plugins.is_empty());
        assert!(!cfg.plugin_enabled("cleanupListOfValues"));
        assert!(!cfg.pretty);
        assert!(cfg.datauri.is_none());
    }

    #[test]
    fn parse_plugin_name_only() {
        let spec = parse_plugin_spec("cleanupListOfValues").unwrap();
        assert_eq!(spec.name, "cleanupListOfValues");
        assert!(spec.params.is_none());
    }

    #[test]
    fn parse_plugin_name_with_json() {
        let spec = parse_plugin_spec(r#"cleanupListOfValues:{"floatPrecision":2}"#).unwrap();
        assert_eq!(spec.name, "cleanupListOfValues");
        assert_eq!(spec.params.unwrap()["floatPrecision"], 2);
    }

    #[test]
    fn parse_plugin_rejects_bad_json() {
        let err = parse_plugin_spec("cleanupListOfValues:{nope}").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--plugin"), "{msg}");
        assert!(msg.contains("cleanupListOfValues"), "{msg}");
    }

    #[test]
    fn parse_param_name_eq_json() {
        let (name, value) =
            parse_param_spec(r#"convertStyleToAttrs={"keepImportant":true}"#).unwrap();
        assert_eq!(name, "convertStyleToAttrs");
        assert_eq!(value["keepImportant"], true);
    }

    #[test]
    fn parse_param_requires_equals() {
        let err = parse_param_spec("cleanupListOfValues").unwrap_err();
        assert!(err.to_string().contains("name=json"), "{err}");
    }

    #[test]
    fn apply_plugin_cli_enables_and_param_wins() {
        let mut cfg = Config::default();
        let plugins = vec![
            parse_plugin_spec(r#"cleanupListOfValues:{"floatPrecision":1}"#).unwrap(),
            parse_plugin_spec("convertStyleToAttrs").unwrap(),
        ];
        apply_plugin_cli(
            &mut cfg,
            &plugins,
            &[r#"cleanupListOfValues={"floatPrecision":4}"#.into()],
        )
        .unwrap();
        assert!(cfg.plugin_enabled("cleanupListOfValues"));
        assert!(cfg.plugin_enabled("convertStyleToAttrs"));
        assert_eq!(
            cfg.plugin_param("cleanupListOfValues").unwrap()["floatPrecision"],
            4
        );
        cfg.skip_plugins.push("cleanupListOfValues".into());
        assert!(!cfg.plugin_enabled("cleanupListOfValues"));
    }

    #[test]
    fn param_alone_does_not_enable() {
        let mut cfg = Config::default();
        apply_plugin_cli(
            &mut cfg,
            &[],
            &[r#"cleanupListOfValues={"floatPrecision":1}"#.into()],
        )
        .unwrap();
        assert!(!cfg.plugin_enabled("cleanupListOfValues"));
        assert_eq!(
            cfg.plugin_param("cleanupListOfValues").unwrap()["floatPrecision"],
            1
        );
    }
}
