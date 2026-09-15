//! minSVG — clean-room Rust SVG optimizer.
//!
//! Inspired by SVGO-shaped plugin passes and by published Rust optimizers
//! (oxvg, svgm, vexy-vsvg, svgcleaner). Their source is not vendored or copied.
//!
//! Pipeline: parse (quick-xml) → plugin list → minify serialize.

mod animation;
mod ast;
mod parse;
mod plugin_minify;
mod plugin_paths;
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
pub use plugin_minify::run_minify;
pub use plugin_paths::{
    convert_path_data, convert_shape_to_path, merge_paths, minify_path_d, PATH_PLUGIN_NAMES,
};
pub use plugin_raster::recompress_embedded_rasters;
pub use plugin_strip::{run_strip, STRIP_PLUGIN_NAMES};
pub use plugins::DEFAULT_PLUGIN_NAMES;

#[derive(Debug, thiserror::Error)]
pub enum OptimizeError {
    #[error("SVG parse error: {0}")]
    Parse(String),
}

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
    /// Plugin names to skip (CLI `--skip`).
    pub skip_plugins: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            preset: Preset::Default,
            animation_aware: true,
            extra_sources: Vec::new(),
            source_name: None,
            skip_plugins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Preset {
    #[default]
    Default,
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
    plugins::run_default_skipping(&mut doc, &plan, &config.skip_plugins);
    let svg = serialize::serialize(&doc);
    let name = config.source_name.as_deref().unwrap_or("input.svg");
    animation::finish_report(&mut report, name, input.len(), &svg, config.animation_aware);
    let plugins_skipped = report.plugins_skipped.clone();
    let plugins_run: Vec<&'static str> = DEFAULT_PLUGIN_NAMES
        .iter()
        .copied()
        .filter(|n| !plugins_skipped.iter().any(|s| s == n))
        .collect();
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
        assert!(out.plugins_skipped.contains(&"mergePaths"));
        assert!(out.plugins_skipped.contains(&"convertShapeToPath"));
    }
}
