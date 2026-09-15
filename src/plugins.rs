//! Plugin pipeline: clean-room strip + minify modules.
//!
//! Implementations live in `plugin_strip.rs` and `plugin_minify.rs` (research
//! from public SVGO / oxvg / vexy contracts — not their source).

use crate::animation::PluginPlan;
use crate::ast::Document;
use crate::{plugin_minify, plugin_strip};

pub const DEFAULT_PLUGIN_NAMES: &[&str] = &[
    "removeDoctype",
    "removeXMLProcInst",
    "removeComments",
    "removeMetadata",
    "removeEditorsNSData",
    "cleanupAttrs",
    "removeEmptyAttrs",
    "removeDeprecatedAttrs",
    "minifyStyles",
    "convertColors",
    "cleanupNumericValues",
    "cleanupIds",
    "removeUselessDefs",
    "removeUselessStrokeAndFill",
    "removeHiddenElems",
    "removeEmptyText",
    "removeDesc",
    "convertEllipseToCircle",
    "convertShapeToPath",
    "collapseGroups",
    "removeEmptyContainers",
    "removeUnusedNS",
    "collapseWhitespace",
    "recompressEmbeddedRasters",
    "convertPathData",
    "mergePaths",
];

pub fn run_default(doc: &mut Document, plan: &PluginPlan) {
    run_default_skipping(doc, plan, &[]);
}

pub fn run_default_skipping(doc: &mut Document, plan: &PluginPlan, skip: &[String]) {
    let on = |name: &str| !skip.iter().any(|s| s.eq_ignore_ascii_case(name));
    if on("removeDoctype") {
        plugin_strip::remove_doctype(doc);
    }
    if on("removeXMLProcInst") {
        plugin_strip::remove_xml_proc_inst(doc);
    }
    if on("removeComments") {
        plugin_strip::remove_comments(doc);
    }
    if on("removeMetadata") {
        plugin_strip::remove_metadata(doc);
    }
    if on("removeEditorsNSData") {
        plugin_strip::remove_editors_ns_data(doc);
    }
    if on("cleanupAttrs") {
        plugin_strip::cleanup_attrs(doc);
    }
    if on("removeEmptyAttrs") {
        plugin_strip::remove_empty_attrs(doc);
    }
    if on("removeDeprecatedAttrs") {
        plugin_strip::remove_deprecated_attrs(doc);
    }
    if on("minifyStyles") {
        plugin_minify::minify_styles(doc);
    }
    if on("convertColors") {
        plugin_minify::convert_colors(doc);
    }
    if on("minifyStyles") {
        plugin_minify::minify_styles(doc);
    }
    if on("cleanupNumericValues") {
        plugin_minify::cleanup_numeric_values(doc);
    }
    if on("cleanupIds") && !plan.skip_cleanup_ids {
        plugin_minify::cleanup_ids(doc);
    }
    if on("removeUselessDefs") {
        plugin_strip::remove_useless_defs(doc, plan.skip_cleanup_ids);
    }
    if on("removeUselessStrokeAndFill") {
        plugin_minify::remove_useless_stroke_and_fill(doc);
    }
    if on("removeHiddenElems") && !plan.skip_cleanup_ids {
        plugin_strip::remove_hidden_elems(doc);
    }
    if on("removeEmptyText") {
        plugin_strip::remove_empty_text(doc);
    }
    if on("removeDesc") {
        plugin_strip::remove_desc(doc);
    }
    if on("convertEllipseToCircle") {
        plugin_minify::convert_ellipse_to_circle(doc);
    }
    if on("convertShapeToPath") && !plan.skip_cleanup_ids {
        crate::plugin_paths::convert_shape_to_path(doc);
    }
    if on("collapseGroups") && !plan.skip_cleanup_ids {
        plugin_strip::collapse_groups(doc);
    }
    if on("removeEmptyContainers") {
        if plan.skip_cleanup_ids {
            plugin_strip::remove_empty_containers_keep_ids(doc);
        } else {
            plugin_strip::remove_empty_containers(doc);
        }
    }
    if on("removeUnusedNS") {
        plugin_strip::remove_unused_ns(doc);
    }
    if on("collapseWhitespace") {
        plugin_minify::collapse_whitespace(doc);
    }
    // Raster payloads + image wrapper. Runs on motion docs (not in
    // MOTION_SKIP_PLUGINS): a tiny SMIL loop must not disable PNG/JPEG
    // recompress or safe wrapper minify.
    if on("recompressEmbeddedRasters") {
        crate::plugin_raster::recompress_embedded_rasters(doc);
    }
    // Icon-only geometry: skip convertPathData / mergePaths on motion
    // (`plan.skip_cleanup_ids`). convertPathData also refuses SMIL-animated `d`.
    if !plan.skip_cleanup_ids {
        if on("convertPathData") {
            crate::plugin_paths::convert_path_data(doc);
        }
        if on("mergePaths") && crate::plugin_paths::merge_paths(doc) && on("convertPathData") {
            crate::plugin_paths::convert_path_data(doc);
        }
    }
}

// TODO(parity): collapseGroups movers, convertTransform bake, LightningCSS
// minifyStyles, full dirty-flag loop (svgm), raster goldens (resvg 1.85+).
