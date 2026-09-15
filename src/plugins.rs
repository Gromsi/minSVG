//! Plugin pipeline: clean-room strip + minify modules.
//!
//! Default IDs live in `plugin_strip.rs` / `plugin_minify.rs`. Opt-in SVGO
//! names (default **off**) are activated only via `--plugin` /
//! [`crate::Config::enable_plugins`].

use crate::animation::PluginPlan;
use crate::ast::Document;
use crate::config::Config;
use crate::plugin_optin_edit::{
    AddAttributesToSvgElementParams, AddClassesToSvgElementParams,
    RemoveAttributesBySelectorParams, RemoveAttrsParams, RemoveElementsByAttrParams,
};
use crate::plugin_prefix::PrefixIdsParams;
use crate::serialize;
use crate::{plugin_minify, plugin_optin_meta, plugin_optin_reuse, plugin_strip};

/// SVGO-style outer cap: re-run the plugin set until serialized size is
/// stable, never more than this many passes.
pub const MAX_MULTIPASS: usize = 10;

pub const DEFAULT_PLUGIN_NAMES: &[&str] = &[
    "removeDoctype",
    "removeXMLProcInst",
    "removeComments",
    "removeMetadata",
    "removeEditorsNSData",
    "cleanupAttrs",
    "removeEmptyAttrs",
    "removeDeprecatedAttrs",
    "mergeStyles",
    "inlineStyles",
    "minifyStyles",
    "convertColors",
    "cleanupNumericValues",
    "removeUnknownsAndDefaults",
    "cleanupIds",
    "removeUselessDefs",
    "removeNonInheritableGroupAttrs",
    "cleanupEnableBackground",
    "removeUselessStrokeAndFill",
    "removeHiddenElems",
    "removeEmptyText",
    "removeDesc",
    "convertEllipseToCircle",
    "convertShapeToPath",
    "moveElemsAttrsToGroup",
    "moveGroupAttrsToElems",
    "collapseGroups",
    "removeEmptyContainers",
    "removeUnusedNS",
    "collapseWhitespace",
    "recompressEmbeddedRasters",
    "convertPathData",
    "convertTransform",
    "mergePaths",
    "sortAttrs",
    "sortDefsChildren",
];

/// SVGO built-ins that stay **off** unless `--plugin NAME` / `enable_plugins`.
///
/// Never overlap [`DEFAULT_PLUGIN_NAMES`]. Sibling modules own the impls;
/// this list is the merge-wire catalog (Task 10).
pub const OPTIN_PLUGIN_NAMES: &[&str] = &[
    "removeViewBox",
    "removeDimensions",
    "removeXMLNS",
    "removeXlink",
    "removeTitle",
    "prefixIds",
    "removeRasterImages",
    "removeOffCanvasPaths",
    "removeScripts",
    "removeStyleElement",
    "reusePaths",
    "convertOneStopGradients",
    "addAttributesToSVGElement",
    "addClassesToSVGElement",
    "removeAttrs",
    "removeAttributesBySelector",
    "removeElementsByAttr",
    "cleanupListOfValues",
    "convertStyleToAttrs",
];

pub fn is_optin_plugin(name: &str) -> bool {
    OPTIN_PLUGIN_NAMES
        .iter()
        .any(|n| n.eq_ignore_ascii_case(name))
}

pub fn run_default_skipping(
    doc: &mut Document,
    plan: &PluginPlan,
    skip: &[String],
    precision: Option<u8>,
) {
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
    if on("mergeStyles") {
        plugin_minify::merge_styles(doc);
    }
    // Icon/static only. Motion docs skip (MOTION_SKIP_PLUGINS).
    if on("inlineStyles") && !plan.skip_cleanup_ids {
        plugin_minify::inline_styles(doc);
    }
    if on("minifyStyles") {
        plugin_minify::minify_styles(doc);
    }
    if on("convertColors") {
        plugin_minify::convert_colors(doc);
    }
    // Second minifyStyles is intentional (convertColors can emit new `style=`
    // values). Not a second DEFAULT_PLUGIN_NAMES entry.
    if on("minifyStyles") {
        plugin_minify::minify_styles(doc);
    }
    if on("cleanupNumericValues") {
        plugin_minify::cleanup_numeric_values_with(
            doc,
            i32::from(precision.unwrap_or(plugin_minify::DEFAULT_NUMERIC_PRECISION)),
        );
    }
    if on("removeUnknownsAndDefaults") {
        plugin_strip::remove_unknowns_and_defaults(doc);
    }
    if on("cleanupIds") && !plan.skip_cleanup_ids {
        plugin_minify::cleanup_ids(doc);
    }
    if on("removeUselessDefs") {
        plugin_strip::remove_useless_defs(doc, plan.skip_cleanup_ids);
    }
    if on("removeNonInheritableGroupAttrs") {
        plugin_strip::remove_non_inheritable_group_attrs(doc);
    }
    if on("cleanupEnableBackground") {
        plugin_strip::cleanup_enable_background(doc);
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
    if on("moveElemsAttrsToGroup") {
        plugin_strip::move_elems_attrs_to_group(doc, plan.skip_cleanup_ids);
    }
    if on("moveGroupAttrsToElems") {
        plugin_strip::move_group_attrs_to_elems(doc, plan.skip_cleanup_ids);
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
    // Icon-only geometry: skip convertPathData / convertTransform / mergePaths
    // on motion (`plan.skip_cleanup_ids`). convertPathData also refuses
    // SMIL-animated `d`; convertTransform refuses animateTransform targets.
    if !plan.skip_cleanup_ids {
        if on("convertPathData") {
            crate::plugin_paths::convert_path_data_with(doc, precision);
        }
        if on("convertTransform") {
            crate::plugin_paths::convert_transform_with(doc, precision);
        }
        if on("mergePaths") && crate::plugin_paths::merge_paths(doc) && on("convertPathData") {
            crate::plugin_paths::convert_path_data_with(doc, precision);
        }
    }
    // Last: paint-neutral gzip order. After path/raster so newly written
    // attrs (`d`, image wrappers) get the same order as source attrs.
    if on("sortAttrs") {
        plugin_strip::sort_attrs(doc);
    }
    if on("sortDefsChildren") {
        plugin_strip::sort_defs_children(doc);
    }
}

/// Run the default plugin set once, or until serialized size stops shrinking
/// (max [`MAX_MULTIPASS`]). Path geometry plugins stay the existing ones —
/// this loop does not rewrite `d`.
///
/// Opt-in names in [`OPTIN_PLUGIN_NAMES`] run after the default set when
/// [`Config::plugin_enabled`] is true. They stay off on `Config::default()`.
pub fn optimize_loop_with(
    doc: &mut Document,
    plan: &PluginPlan,
    skip: &[String],
    enable: &[String],
    multipass: bool,
    precision: Option<u8>,
    config: Option<&Config>,
) -> String {
    let limit = if multipass { MAX_MULTIPASS } else { 1 };
    let mut best = String::new();
    let mut prev_size = usize::MAX;
    for _ in 0..limit {
        run_default_skipping(doc, plan, skip, precision);
        run_optin_enabled(doc, plan, skip, enable, config);
        let next = serialize::serialize(doc);
        if next.len() >= prev_size {
            break;
        }
        prev_size = next.len();
        best = next;
    }
    best
}

/// Activate named opt-in passes. Empty `enable` (the default) is a no-op.
pub fn run_optin_enabled(
    doc: &mut Document,
    _plan: &PluginPlan,
    skip: &[String],
    enable: &[String],
    config: Option<&Config>,
) {
    let on = |name: &str| {
        if skip.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            return false;
        }
        if let Some(cfg) = config {
            if cfg.plugin_enabled(name) {
                return true;
            }
        }
        enable.iter().any(|s| s.eq_ignore_ascii_case(name))
    };

    for name in plugin_optin_meta::OPTIN_META_PLUGIN_NAMES {
        if on(name) {
            for node in &mut doc.nodes {
                plugin_optin_meta::run(name, node);
            }
        }
    }

    if on("prefixIds") {
        let params = prefix_ids_params(config);
        let path = config.and_then(|c| c.source_name.as_deref());
        crate::plugin_prefix::prefix_ids_with(doc, &params, path);
    }

    if on("removeRasterImages") {
        crate::plugin_optin_danger::run_remove_raster_images(doc);
    }
    if on("removeOffCanvasPaths") {
        let skip_motion = config.map(|c| c.animation_aware).unwrap_or(true);
        crate::plugin_optin_danger::run_remove_off_canvas_paths_with(doc, skip_motion);
    }
    if on("removeScripts") {
        crate::plugin_optin_danger::run_remove_scripts(doc);
    }
    if on("removeStyleElement") {
        let skip_motion = config.map(|c| c.animation_aware).unwrap_or(true);
        crate::plugin_optin_danger::run_remove_style_element_with(doc, skip_motion);
    }

    let reuse_on: Vec<String> = plugin_optin_reuse::OPTIN_REUSE_PLUGIN_NAMES
        .iter()
        .copied()
        .filter(|name| on(name))
        .map(str::to_string)
        .collect();
    if !reuse_on.is_empty() {
        plugin_optin_reuse::run_optin_reuse(doc, &reuse_on);
    }

    if on("addAttributesToSVGElement") {
        crate::plugin_optin_edit::run_add_attributes_to_svg_element(
            doc,
            &add_attributes_params(config),
        );
    }
    if on("addClassesToSVGElement") {
        crate::plugin_optin_edit::run_add_classes_to_svg_element(doc, &add_classes_params(config));
    }
    if on("removeAttrs") {
        crate::plugin_optin_edit::run_remove_attrs(doc, &remove_attrs_params(config));
    }
    if on("removeAttributesBySelector") {
        crate::plugin_optin_edit::run_remove_attributes_by_selector(
            doc,
            &remove_attrs_by_selector_params(config),
        );
    }
    if on("removeElementsByAttr") {
        crate::plugin_optin_edit::run_remove_elements_by_attr(
            doc,
            &remove_elements_by_attr_params(config),
        );
    }

    // List/style named passes also have lib.rs hooks; running here keeps
    // multipass + `--plugin` in one pipeline. Empty enable is still a no-op.
    if on("convertStyleToAttrs") {
        if let Some(cfg) = config {
            crate::plugin_optin_lists::run_enabled_style_pass(doc, cfg);
        } else {
            crate::plugin_optin_lists::convert_style_to_attrs(doc);
        }
    }
    if on("cleanupListOfValues") {
        if let Some(cfg) = config {
            crate::plugin_optin_lists::run_enabled_list_pass(doc, cfg);
        } else {
            crate::plugin_optin_lists::cleanup_list_of_values(doc);
        }
    }
}

fn prefix_ids_params(config: Option<&Config>) -> PrefixIdsParams {
    config.map(PrefixIdsParams::from_config).unwrap_or_default()
}

fn add_attributes_params(config: Option<&Config>) -> AddAttributesToSvgElementParams {
    config
        .map(AddAttributesToSvgElementParams::from_config)
        .unwrap_or_default()
}

fn add_classes_params(config: Option<&Config>) -> AddClassesToSvgElementParams {
    config
        .map(AddClassesToSvgElementParams::from_config)
        .unwrap_or_default()
}

fn remove_attrs_params(config: Option<&Config>) -> RemoveAttrsParams {
    config
        .map(RemoveAttrsParams::from_config)
        .unwrap_or_default()
}

fn remove_attrs_by_selector_params(config: Option<&Config>) -> RemoveAttributesBySelectorParams {
    config
        .map(RemoveAttributesBySelectorParams::from_config)
        .unwrap_or_default()
}

fn remove_elements_by_attr_params(config: Option<&Config>) -> RemoveElementsByAttrParams {
    config
        .map(RemoveElementsByAttrParams::from_config)
        .unwrap_or_default()
}

// TODO(parity): LightningCSS minifyStyles, svgm dirty-flag inner loop,
// raster goldens (resvg 1.85+).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::PluginPlan;
    use crate::parse;

    #[test]
    fn default_path_wires_group_attrs_and_enable_background() {
        let mut seen = std::collections::BTreeSet::new();
        for name in DEFAULT_PLUGIN_NAMES {
            assert!(
                seen.insert(*name),
                "duplicate DEFAULT_PLUGIN_NAMES id: {name}"
            );
        }
        for required in [
            "mergeStyles",
            "inlineStyles",
            "convertTransform",
            "moveElemsAttrsToGroup",
            "moveGroupAttrsToElems",
            "removeUnknownsAndDefaults",
            "sortAttrs",
            "sortDefsChildren",
            "removeNonInheritableGroupAttrs",
            "cleanupEnableBackground",
        ] {
            assert_eq!(
                DEFAULT_PLUGIN_NAMES
                    .iter()
                    .filter(|n| **n == required)
                    .count(),
                1,
                "{required} must be wired once"
            );
        }
        assert!(!DEFAULT_PLUGIN_NAMES.contains(&"removeViewBox"));
        assert!(!DEFAULT_PLUGIN_NAMES.contains(&"removeTitle"));
        assert!(OPTIN_PLUGIN_NAMES.contains(&"removeViewBox"));
        assert!(OPTIN_PLUGIN_NAMES.contains(&"removeTitle"));
        for name in OPTIN_PLUGIN_NAMES {
            assert!(
                !DEFAULT_PLUGIN_NAMES.contains(name),
                "{name} must stay opt-in (default OFF)"
            );
        }

        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" viewBox="0 0 10 10" enable-background="new 0 0 10 10">"##,
            r##"<title>Keep</title>"##,
            r##"<g flood-color="lime" fill="#0a0"><circle r="2" fill-rule="nonzero"/></g>"##,
            "</svg>",
        );
        let mut doc = parse::parse_svg(input).unwrap();
        run_default_skipping(&mut doc, &PluginPlan::default(), &[], None);
        let out = serialize::serialize(&doc);
        assert!(out.contains("viewBox"), "{out}");
        assert!(out.contains("<title>"), "{out}");
        assert!(out.contains("role=\"img\""), "{out}");
        assert!(out.contains("xmlns="), "{out}");
        assert!(!out.contains("enable-background"), "{out}");
        assert!(!out.contains("flood-color"), "{out}");
        assert!(!out.contains("fill-rule"), "{out}");
        assert!(out.contains("#0a0"), "{out}");
    }

    #[test]
    fn default_path_sort_is_stable_across_source_order() {
        let a = concat!(
            r##"<svg viewBox="0 0 10 10" xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs>"##,
            r##"<clipPath id="c"><circle r="1"/></clipPath>"##,
            r##"<linearGradient id="g1"><stop offset="0" stop-color="#f00"/></linearGradient>"##,
            r##"<mask id="m"><circle r="1"/></mask>"##,
            r##"<linearGradient id="g2"><stop offset="1" stop-color="#00f"/></linearGradient>"##,
            r##"</defs>"##,
            r##"<circle fill="url(#g1)" r="4" cy="5" cx="5" clip-path="url(#c)" mask="url(#m)"/>"##,
            r##"<circle fill="url(#g2)" r="2" cy="5" cx="5"/>"##,
            "</svg>",
        );
        let b = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<defs>"##,
            r##"<mask id="m"><circle r="1"/></mask>"##,
            r##"<linearGradient id="g2"><stop offset="1" stop-color="#00f"/></linearGradient>"##,
            r##"<linearGradient id="g1"><stop offset="0" stop-color="#f00"/></linearGradient>"##,
            r##"<clipPath id="c"><circle r="1"/></clipPath>"##,
            r##"</defs>"##,
            r##"<circle cx="5" cy="5" r="4" fill="url(#g1)" mask="url(#m)" clip-path="url(#c)"/>"##,
            r##"<circle cx="5" cy="5" r="2" fill="url(#g2)"/>"##,
            "</svg>",
        );
        let mut da = parse::parse_svg(a).unwrap();
        let mut db = parse::parse_svg(b).unwrap();
        run_default_skipping(&mut da, &PluginPlan::default(), &[], None);
        run_default_skipping(&mut db, &PluginPlan::default(), &[], None);
        let out_a = serialize::serialize(&da);
        let out_b = serialize::serialize(&db);
        assert_eq!(out_a, out_b, "a={out_a}\nb={out_b}");
        assert!(
            out_a.contains(r#"xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10""#)
                || out_a.find("xmlns=").unwrap() < out_a.find("viewBox=").unwrap(),
            "{out_a}"
        );

        let skip = ["sortAttrs".into(), "sortDefsChildren".into()];
        let mut sa = parse::parse_svg(a).unwrap();
        let mut sb = parse::parse_svg(b).unwrap();
        run_default_skipping(&mut sa, &PluginPlan::default(), &skip, None);
        run_default_skipping(&mut sb, &PluginPlan::default(), &skip, None);
        assert_ne!(
            serialize::serialize(&sa),
            serialize::serialize(&sb),
            "without sort plugins, source order must survive"
        );
    }

    #[test]
    fn default_path_hoists_shared_fill_keeps_view_box() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<g>"##,
            r##"<circle fill="#ff0000" r="1" cx="2" cy="2"/>"##,
            r##"<circle fill="#ff0000" r="1" cx="7" cy="7"/>"##,
            r##"</g></svg>"##,
        );
        let mut doc = parse::parse_svg(input).unwrap();
        run_default_skipping(&mut doc, &PluginPlan::default(), &[], None);
        let out = serialize::serialize(&doc);
        assert!(out.contains("viewBox"), "{out}");
        assert!(!DEFAULT_PLUGIN_NAMES.contains(&"removeViewBox"));
        let fill_hits = out.matches("fill=").count();
        assert_eq!(
            fill_hits, 1,
            "shared fill should hoist onto the group: {out}"
        );
        assert!(
            out.contains("fill=\"red\"")
                || out.contains("fill=\"#f00\"")
                || out.contains("fill=\"#ff0000\""),
            "{out}"
        );

        let motion = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<g id="layer" transform="translate(1 2)">"##,
            r##"<circle r="2" fill="#0a0"/>"##,
            r##"</g></svg>"##,
        );
        let mut motion_doc = parse::parse_svg(motion).unwrap();
        run_default_skipping(
            &mut motion_doc,
            &PluginPlan {
                skip_cleanup_ids: true,
            },
            &[],
            None,
        );
        let mout = serialize::serialize(&motion_doc);
        assert!(mout.contains("viewBox"), "{mout}");
        assert!(
            mout.contains("id=\"layer\""),
            "motion-id group stays: {mout}"
        );
        assert!(
            mout.contains("transform=\"translate(1 2)\""),
            "motion-id transform stays on the group: {mout}"
        );
    }

    #[test]
    fn optin_remove_viewbox_drops_only_when_enabled() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" viewBox="0 0 24 24" width="24" height="24">"##,
            r##"<title>Keep</title><path d="M4 12h16"/></svg>"##,
        );
        let mut off = parse::parse_svg(input).unwrap();
        run_default_skipping(&mut off, &PluginPlan::default(), &[], None);
        let kept = serialize::serialize(&off);
        assert!(kept.contains("viewBox"), "{kept}");
        assert!(kept.contains("<title>"), "{kept}");
        assert!(kept.contains("role=\"img\""), "{kept}");

        let mut on = parse::parse_svg(input).unwrap();
        run_optin_enabled(
            &mut on,
            &PluginPlan::default(),
            &[],
            &["removeViewBox".into()],
            None,
        );
        let dropped = serialize::serialize(&on);
        assert!(!dropped.contains("viewBox"), "{dropped}");
        assert!(dropped.contains("role=\"img\""), "{dropped}");
        assert!(dropped.contains("<title>"), "{dropped}");
        assert!(dropped.contains("width=\"24\""), "{dropped}");
    }
}
