//! minSVG — clean-room Rust SVG optimizer.
//!
//! Inspired by SVGO-shaped plugin passes and by published Rust optimizers
//! (oxvg, svgm, vexy-vsvg, svgcleaner). Their source is not vendored or copied.
//!
//! Pipeline: parse (quick-xml) → plugin list → minify serialize.
//!
//! Optional features stay off the default rlib: `serve` / `mcp` are bin-only.

mod animation;
mod ast;
mod config;
mod config_file;
mod parse;
mod plugin_minify;
mod plugin_optin_danger;
mod plugin_optin_edit;
mod plugin_optin_lists;
mod plugin_optin_meta;
mod plugin_optin_reuse;
mod plugin_paths;
mod plugin_prefix;
mod plugin_raster;
mod plugin_strip;
mod plugins;
mod serialize;

pub use animation::{
    count_id_attrs, detect_document, finish_report, format_summary, parse_smil_clock_value,
    rewrite_smil_clock_value, scan_sources_for_id_refs, AnimationKind, AnimationReport, PluginPlan,
    SmilSyncRef, MOTION_SKIP_PLUGINS,
};
pub use ast::{Document, Element, Node};
pub use config::{
    apply_plugin_cli, parse_param_spec, parse_plugin_spec, Config, PluginParamError, PluginSpec,
    Preset,
};
pub use config_file::{
    config_from_cwd_and_cli, config_from_dir_and_cli, config_from_path_and_cli,
    load_config_from_cwd, load_config_from_dir, load_config_from_path, load_config_path,
    merge_config, parse_config_json, parse_config_toml, parse_config_yaml, CliOverrides,
    ConfigFileError, FileConfig, FilePlugin, Js2SvgConfig, LoadedConfig, CONFIG_JSON, CONFIG_TOML,
    CONFIG_YAML, CONFIG_YML, SVGO_CONFIG_JSON, SVGO_CONFIG_YAML, SVGO_CONFIG_YML,
};
pub use plugin_minify::run_minify;
pub use plugin_optin_danger::{
    run_optin_danger, run_remove_off_canvas_paths, run_remove_off_canvas_paths_node,
    run_remove_off_canvas_paths_with, run_remove_raster_images, run_remove_raster_images_node,
    run_remove_scripts, run_remove_scripts_node, run_remove_style_element,
    run_remove_style_element_node, run_remove_style_element_with, OPTIN_DANGER_PLUGIN_NAMES,
};
pub use plugin_optin_edit::{
    run_add_attributes_to_svg_element, run_add_classes_to_svg_element, run_enabled_edit_pass,
    run_remove_attributes_by_selector, run_remove_attrs, run_remove_elements_by_attr,
    AddAttributesToSvgElementParams, AddClassesToSvgElementParams,
    RemoveAttributesBySelectorParams, RemoveAttrsParams, RemoveElementsByAttrParams,
    SelectorAttrRule, OPTIN_EDIT_PLUGIN_NAMES,
};
pub use plugin_optin_lists::{
    cleanup_list_of_values, cleanup_list_of_values_with, convert_style_to_attrs,
    convert_style_to_attrs_with, run_cleanup_list_of_values, run_cleanup_list_of_values_with,
    run_convert_style_to_attrs, run_convert_style_to_attrs_with, run_enabled_list_pass,
    run_enabled_style_pass, CleanupListOfValuesParams, ConvertStyleToAttrsParams,
    LIST_OPTIN_PLUGIN_NAMES,
};
pub use plugin_optin_meta::{
    remove_dimensions, remove_title, remove_view_box, remove_xlink, remove_xlink_with,
    remove_xmlns, run_optin_meta, run_remove_dimensions, run_remove_title, run_remove_view_box,
    run_remove_xlink, run_remove_xlink_with, run_remove_xmlns, OPTIN_META_PLUGIN_NAMES,
};
pub use plugin_optin_reuse::{
    convert_one_stop_gradients, convert_one_stop_gradients_with, reuse_paths, reuse_paths_with,
    run_convert_one_stop_gradients, run_optin_reuse, run_reuse_paths, OPTIN_REUSE_PLUGIN_NAMES,
};
pub use plugin_paths::{
    convert_path_data, convert_path_data_with, convert_shape_to_path, convert_transform,
    convert_transform_with, merge_paths, minify_path_d, minify_path_d_with, minify_transform,
    minify_transform_with, PATH_PLUGIN_NAMES,
};
pub use plugin_prefix::{prefix_ids, prefix_ids_with, PrefixIdsParams, PREFIX_IDS_NAME};
pub use plugin_raster::recompress_embedded_rasters;
pub use plugin_strip::{run_strip, STRIP_PLUGIN_NAMES};
pub use plugins::{is_optin_plugin, DEFAULT_PLUGIN_NAMES, OPTIN_PLUGIN_NAMES};
pub use serialize::{
    encode_svg_datauri, serialize, serialize_pretty, serialize_with, DataUri, SerializeOptions,
};

#[derive(Debug, thiserror::Error)]
pub enum OptimizeError {
    #[error("SVG parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Clone)]
pub struct OptimizeOutput {
    pub svg: String,
    pub plugins_run: Vec<&'static str>,
    pub plugins_skipped: Vec<&'static str>,
    pub animation: AnimationReport,
    pub summary: String,
}

/// Optimize raw SVG bytes with the MVP default plugin set.
pub fn optimize(input: &[u8], config: &Config) -> Result<OptimizeOutput, OptimizeError> {
    optimize_with(input, config)
}

pub fn optimize_with(input: &[u8], config: &Config) -> Result<OptimizeOutput, OptimizeError> {
    let text = std::str::from_utf8(input)
        .map_err(|e| OptimizeError::Parse(format!("input is not UTF-8: {e}")))?;
    optimize_str_with(text, config)
}

pub fn optimize_str(input: &str) -> Result<OptimizeOutput, OptimizeError> {
    optimize_str_with(input, &Config::default())
}

pub fn optimize_str_with(input: &str, config: &Config) -> Result<OptimizeOutput, OptimizeError> {
    let mut doc = parse::parse_svg(input)?;
    let mut report = animation::detect_document(&doc, &config.extra_sources);
    let plan = animation::plan_from_report(&report, config.animation_aware);
    plugin_optin_lists::run_enabled_style_pass(&mut doc, config);
    plugin_optin_lists::run_enabled_list_pass(&mut doc, config);
    let compact = plugins::optimize_loop_with(
        &mut doc,
        &plan,
        &config.skip_plugins,
        &config.enable_plugins,
        config.multipass,
        config.precision,
        Some(config),
    );
    let name = config.source_name.as_deref().unwrap_or("input.svg");
    animation::finish_report(
        &mut report,
        name,
        input.len(),
        &compact,
        config.animation_aware,
    );
    let mut svg = if config.pretty {
        serialize::serialize_with(
            &doc,
            serialize::SerializeOptions {
                pretty: true,
                indent: config.indent,
            },
        )
    } else {
        compact
    };
    if let Some(kind) = config.datauri {
        svg = serialize::encode_svg_datauri(&svg, kind);
    }
    let plugins_skipped = report.plugins_skipped.clone();
    let mut plugins_run: Vec<&'static str> = DEFAULT_PLUGIN_NAMES
        .iter()
        .copied()
        .filter(|n| !plugins_skipped.iter().any(|s| s == n))
        .collect();
    for name in OPTIN_PLUGIN_NAMES {
        if config.plugin_enabled(name) && !plugins_run.iter().any(|s| *s == *name) {
            plugins_run.push(*name);
        }
    }
    let summary = report.summary.clone();
    Ok(OptimizeOutput {
        svg,
        plugins_run,
        plugins_skipped,
        animation: report,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(s: &str) -> String {
        optimize_str(s).unwrap().svg
    }

    #[test]
    fn strips_comments_doctype_and_xml_decl() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<svg xmlns="http://www.w3.org/2000/svg">
  <!-- note -->
  <circle cx="10" cy="10" r="5"/>
</svg>"#;
        let out = opt(input);
        assert!(!out.contains("<?xml"));
        assert!(!out.contains("DOCTYPE"));
        assert!(!out.contains("<!--"));
        assert!(out.contains("<circle"));
    }

    #[test]
    fn strips_metadata_and_editor_ns() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" inkscape:version="1.3">
  <metadata><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description/></rdf:RDF></metadata>
  <sodipodi:namedview inkscape:cx="0"/>
  <g inkscape:label="Layer 1"><circle cx="4" cy="4" r="2"/></g>
</svg>"#;
        let out = opt(input);
        assert!(!out.contains("metadata"));
        assert!(!out.contains("namedview"));
        assert!(!out.contains("inkscape"));
        assert!(out.contains("<circle"));
    }

    #[test]
    fn removes_empty_groups_and_empty_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
            <g id=\"unused\"><g></g></g>\
            <rect x=\"0\" y=\"0\" width=\"10\" height=\"10\" class=\"\" fill=\"#000000\"/>\
            </svg>";
        let out = opt(input);
        assert!(!out.contains("<g"));
        assert!(!out.contains("class="));
        assert!(!out.contains("fill="));
        // Axis-aligned rect becomes <path> (convertShapeToPath); keep a shape either way.
        assert!(out.contains("<rect") || out.contains("<path"), "{out}");
    }

    #[test]
    fn converts_colors_and_minifies_style() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
            <rect x=\"0\" y=\"0\" width=\"10\" height=\"10\" fill=\"rgb(255, 255, 255)\" stroke=\"#FF0000\" style=\"  fill-opacity: 1 ;  \"/>\
            </svg>";
        let out = opt(input);
        assert!(out.contains("fill=\"#fff\""), "{out}");
        // Shortest equivalent may be the CSS name (`red`) rather than `#f00`.
        assert!(
            out.contains("stroke=\"red\"") || out.contains("stroke=\"#f00\""),
            "{out}"
        );
    }

    #[test]
    fn preserves_xlink_href_use_and_paint() {
        let input = concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">",
            "<defs><g id=\"charge\"><path d=\"M0 0h2v2H0z\" fill=\"#fff\"/></g>",
            "<linearGradient id=\"g1\"><stop offset=\"0\" stop-color=\"#f00\"/></linearGradient></defs>",
            "<use xlink:href=\"#charge\"/>",
            "<rect width=\"10\" height=\"10\" fill=\"url(#g1)\"/></svg>"
        );
        let out = opt(input);
        assert!(out.contains("id=\"charge\""), "{out}");
        assert!(out.contains("#charge"), "{out}");
        assert!(out.contains("id=\"g1\""), "{out}");
        assert!(out.contains("url(#g1)"), "{out}");
        assert!(out.contains("xmlns:xlink"), "{out}");
    }

    #[test]
    fn preserves_url_paint_id_case() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>\
            <linearGradient id=\"poolFill\"><stop offset=\"0\" stop-color=\"#fba\"/></linearGradient>\
            </defs><ellipse fill=\"url(#poolFill)\" cx=\"10\" cy=\"10\" rx=\"8\" ry=\"4\"/></svg>";
        let out = opt(input);
        assert!(out.contains("id=\"poolFill\""), "{out}");
        assert!(out.contains("fill=\"url(#poolFill)\""), "{out}");
        assert!(!out.contains("url(#poolfill)"), "{out}");
    }

    #[test]
    fn keeps_referenced_ids_drops_unused() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>\
            <linearGradient id=\"g1\"><stop offset=\"0\" stop-color=\"#fff\"/><stop offset=\"1\" stop-color=\"#000\"/></linearGradient>\
            <linearGradient id=\"unused\"><stop offset=\"0\" stop-color=\"red\"/></linearGradient>\
            </defs><rect width=\"10\" height=\"10\" fill=\"url(#g1)\"/></svg>";
        let out = opt(input);
        assert!(out.contains("id=\"g1\""), "{out}");
        assert!(!out.contains("unused"), "{out}");
    }

    #[test]
    fn round_trip_simple_icon() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 12h16"/></svg>"#;
        let out = opt(input);
        assert!(out.starts_with("<svg"));
        assert!(out.contains("viewBox=\"0 0 24 24\""));
        assert!(out.contains("<path"));
    }

    #[test]
    fn keeps_role_img_on_logo_like_svg() {
        let input = r#"<svg role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><title>GitHub</title><path d="M12 .297c-6.63 0-12 5.373-12 12"/></svg>"#;
        let out = opt(input);
        assert!(out.contains("role=\"img\""), "{out}");
        assert!(out.contains("viewBox=\"0 0 24 24\""), "{out}");
        assert!(out.contains("xmlns="), "{out}");
        assert!(out.contains("<title>GitHub</title>"), "{out}");
        assert!(!out.contains("M12.297"), "{out}");
    }

    #[test]
    fn motion_keeps_verbose_d_and_smil_clock() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg">
            <path id="mouth" d="M 156.0000 106.0000 q 10 5 18 0">
              <animate id="chew" attributeName="d" begin="0s; chew.end+0.14s" dur="0.38s"
                values="M 156.0000 106.0000 q 10 5 18 0; M 156 106 q 10 1 18 0" repeatCount="indefinite"/>
            </path>
          </svg>"#;
        let out = optimize_str(input).unwrap();
        assert!(out.svg.contains("156.0000"), "{}", out.svg);
        assert!(out.svg.contains("chew.end+0.14s"), "{}", out.svg);
        assert!(out.svg.contains("dur=\"0.38s\""), "{}", out.svg);
        assert!(out.plugins_skipped.contains(&"convertPathData"));
        assert!(out.plugins_skipped.contains(&"convertTransform"));
        assert!(out.plugins_skipped.contains(&"mergePaths"));
        assert!(out.plugins_skipped.contains(&"convertShapeToPath"));

        let mut cfg = Config::default();
        cfg.precision = Some(2);
        let rounded = optimize_str_with(input, &cfg).unwrap();
        assert!(
            rounded.svg.contains("156.0000"),
            "precision must not rewrite motion d: {}",
            rounded.svg
        );
        assert!(rounded.plugins_skipped.contains(&"convertPathData"));
    }

    #[test]
    fn multipass_verbose_svg_is_not_larger_than_one_pass() {
        let input = concat!(
            r##"<?xml version="1.0" encoding="UTF-8"?>"##,
            r##"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">"##,
            r##"<svg xmlns="http://www.w3.org/2000/svg" version="1.1">"##,
            "<!-- leftover -->",
            r##"<metadata><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description/></rdf:RDF></metadata>"##,
            r##"<g><g id="unused"><g>"##,
            r##"<rect x="0.0000" y="0.0000" width="10.0000" height="10.0000" fill="#000000" class="" style="  fill-opacity: 1 ;  "/>"##,
            r##"</g></g></g></svg>"##,
        );
        let one = optimize_str_with(input, &Config::default()).unwrap();
        let multi = optimize_str_with(
            input,
            &Config {
                multipass: true,
                ..Config::default()
            },
        )
        .unwrap();
        assert!(
            multi.svg.len() <= one.svg.len(),
            "multipass {}B > one-pass {}B\nmulti={}\none={}",
            multi.svg.len(),
            one.svg.len(),
            multi.svg,
            one.svg
        );
        assert!(multi.svg.contains("<svg"), "{}", multi.svg);
        // Path minify is unchanged: still the existing convertShapeToPath /
        // convertPathData pair, not a new geometry rewrite in this loop.
        assert!(
            multi.svg.contains("<rect") || multi.svg.contains("<path"),
            "{}",
            multi.svg
        );
    }

    #[test]
    fn convert_transform_shortens_and_does_not_bake_d() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<g transform="translate(10.000, 0.000)">'##,
            r##"<path d="M0 0h4"/>"##,
            "</g></svg>",
        );
        let out = opt(input);
        assert!(
            out.contains("translate(10)"),
            "transform not shortened: {out}"
        );
        assert!(!out.contains("10.000"), "{out}");
        assert!(
            !out.contains("M10") && !out.contains("m10"),
            "transform was baked into d: {out}"
        );
        assert!(out.contains("<path"), "{out}");
    }

    #[test]
    fn pretty_indents_after_compact_plugins() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<g><circle cx="10" cy="10" r="5"/></g>"##,
            "</svg>",
        );
        let compact = optimize_str(input).unwrap();
        assert!(
            !compact.svg.contains("\n    <"),
            "default emit must stay compact: {}",
            compact.svg
        );
        let pretty = optimize_str_with(
            input,
            &Config {
                pretty: true,
                ..Config::default()
            },
        )
        .unwrap();
        assert!(pretty.svg.contains('\n'), "{}", pretty.svg);
        assert!(
            pretty.svg.contains("\n    <") || pretty.svg.contains("\n  <"),
            "{}",
            pretty.svg
        );
        assert!(
            pretty.svg.contains("<circle") || pretty.svg.contains("<path"),
            "{}",
            pretty.svg
        );
        assert_eq!(pretty.summary, compact.summary);
    }

    #[test]
    fn datauri_base64_and_enc_wrap_svg() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><circle cx="1" cy="1" r="1"/></svg>"#;
        let b64 = optimize_str_with(
            input,
            &Config {
                datauri: Some(DataUri::Base64),
                ..Config::default()
            },
        )
        .unwrap();
        assert!(
            b64.svg.starts_with("data:image/svg+xml;base64,"),
            "{}",
            b64.svg
        );
        assert!(!b64.svg.contains("<svg"), "{}", b64.svg);
        assert!(b64.summary.contains("preserved"), "{}", b64.summary);

        let enc = optimize_str_with(
            input,
            &Config {
                datauri: Some(DataUri::Enc),
                pretty: true,
                ..Config::default()
            },
        )
        .unwrap();
        assert!(enc.svg.starts_with("data:image/svg+xml,"), "{}", enc.svg);
        assert!(enc.svg.contains("%3Csvg"), "{}", enc.svg);
        assert!(
            enc.svg.contains("%0A") || enc.svg.contains("%20"),
            "{}",
            enc.svg
        );
    }
}
