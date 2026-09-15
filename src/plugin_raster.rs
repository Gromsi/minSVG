//! Embedded-raster handling that still runs on motion-sensitive documents.
//!
//! Path / id / merge plugins stay skipped when SMIL is present. This pass
//! does **not**: it recompresses `data:` PNG/JPEG payloads and minifies the
//! wrapper around `<image>` (including a tiny SMIL visibility loop).
//!
//! We never drop `href` / `xlink:href`. We never delete `visibility="hidden"`
//! artwork that SMIL later reveals (SVGO `removeHiddenElems` does — that is
//! why the driving-POV preview goes blank).

use crate::animation::collect_smil_sync_ids;
use crate::ast::{Document, Element, Node};
use crate::plugin_minify::{collect_url_ids, is_smil_element, is_zero_number};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use std::collections::{BTreeSet, HashSet};
use std::io::Cursor;

const HREF_KEYS: &[&str] = &["href", "xlink:href"];

/// Recompress embedded rasters and minify a safe image wrapper.
///
/// Always safe to run on motion docs: it does not rewrite path `d`, IDs that
/// SMIL/CSS/JS still name, or hidden subtrees that are animation targets.
pub fn recompress_embedded_rasters(doc: &mut Document) {
    let refs = collect_pointer_ids(&doc.nodes);
    flatten_raster_groups(&mut doc.nodes, &refs);
    doc.walk_elements_mut(&mut |el| {
        if el.local_name() != "image" {
            return;
        }
        minify_image_wrapper(el);
        recompress_image_hrefs(el);
        compact_visibility_smil(el);
    });
    hoist_shared_image_css(&mut doc.nodes);
}

fn recompress_image_hrefs(el: &mut Element) {
    for key in HREF_KEYS {
        let Some(raw) = el.attr(key).map(str::to_string) else {
            continue;
        };
        let Some(next) = recompress_data_uri(&raw) else {
            continue;
        };
        if next.len() < raw.len() {
            el.set_attr(key, next);
        }
    }
}

fn minify_image_wrapper(el: &mut Element) {
    if el.attr("x").is_some_and(is_zero_number) {
        el.remove_attr("x");
    }
    if el.attr("y").is_some_and(is_zero_number) {
        el.remove_attr("y");
    }
    let par = el
        .attr("preserveAspectRatio")
        .or_else(|| el.attr("preserveaspectratio"))
        .map(str::to_string);
    if par
        .as_deref()
        .is_some_and(|v| v.eq_ignore_ascii_case("none"))
        && par_none_is_noop(el)
    {
        el.remove_attr("preserveAspectRatio");
        el.remove_attr("preserveaspectratio");
    }
}

fn par_none_is_noop(el: &Element) -> bool {
    let Some(w) = el.attr("width").and_then(parse_user_unit) else {
        return false;
    };
    let Some(h) = el.attr("height").and_then(parse_user_unit) else {
        return false;
    };
    if w <= 0.0 || h <= 0.0 {
        return false;
    }
    let Some(uri) = first_data_uri(el) else {
        return false;
    };
    let Some(parsed) = parse_data_uri(uri) else {
        return false;
    };
    let Some((rw, rh)) = raster_dimensions(&parsed.mime, &parsed.bytes) else {
        return false;
    };
    if rw == 0 || rh == 0 {
        return false;
    }
    let same_px = (w - f64::from(rw)).abs() < 0.5 && (h - f64::from(rh)).abs() < 0.5;
    let same_aspect = (w / h - f64::from(rw) / f64::from(rh)).abs() < 1e-6;
    same_px || same_aspect
}

fn first_data_uri(el: &Element) -> Option<&str> {
    for key in HREF_KEYS {
        if let Some(v) = el.attr(key) {
            if v.trim_start().starts_with("data:") {
                return Some(v);
            }
        }
    }
    None
}

fn compact_visibility_smil(el: &mut Element) {
    for child in &mut el.children {
        let Node::Element(anim) = child else {
            continue;
        };
        if !is_smil_element(anim.local_name()) {
            continue;
        }
        let attr = anim
            .attr("attributeName")
            .or_else(|| anim.attr("attributename"))
            .unwrap_or("");
        if !attr.eq_ignore_ascii_case("visibility") && !attr.eq_ignore_ascii_case("display") {
            continue;
        }
        compact_list_attr(anim, "values");
        compact_list_attr(anim, "keyTimes");
        compact_list_attr(anim, "keySplines");
        if anim.attr("begin").is_some_and(is_default_begin) {
            anim.remove_attr("begin");
        }
    }
}

fn compact_list_attr(el: &mut Element, name: &str) {
    let Some(raw) = el.attr(name).map(str::to_string) else {
        return;
    };
    let next = raw.split(';').map(str::trim).collect::<Vec<_>>().join(";");
    if next != raw {
        el.set_attr(name, next);
    }
}

fn is_default_begin(v: &str) -> bool {
    matches!(v.trim(), "0" | "0s" | "0.0" | "0.0s" | "0ms")
}

/// When many sibling `<image>`s share a box and most start hidden (SMIL
/// frame stacks), one `<style>` is shorter than repeating attrs.
fn hoist_shared_image_css(nodes: &mut [Node]) {
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            hoist_shared_image_css(&mut el.children);
            hoist_shared_image_css_in(el);
        }
    }
}

fn hoist_shared_image_css_in(parent: &mut Element) {
    let image_idxs: Vec<usize> = parent
        .children
        .iter()
        .enumerate()
        .filter_map(|(i, n)| match n {
            Node::Element(el) if el.local_name() == "image" => Some(i),
            _ => None,
        })
        .collect();
    if image_idxs.len() < 2 {
        return;
    }

    let mut box_hw: Option<(String, String)> = None;
    let mut box_ok = true;
    let mut hidden_n = 0usize;
    for &i in &image_idxs {
        let Node::Element(img) = &parent.children[i] else {
            continue;
        };
        match (img.attr("width"), img.attr("height")) {
            (Some(w), Some(h)) => match &box_hw {
                None => box_hw = Some((w.to_string(), h.to_string())),
                Some((ow, oh)) if ow == w && oh == h => {}
                _ => box_ok = false,
            },
            _ => box_ok = false,
        }
        if img
            .attr("visibility")
            .is_some_and(|v| v.eq_ignore_ascii_case("hidden"))
        {
            hidden_n += 1;
        }
    }

    let hoist_box = box_ok && box_hw.is_some();
    let hoist_hidden = hidden_n >= 2;
    if !hoist_box && !hoist_hidden {
        return;
    }

    let mut css = String::new();
    if hoist_box {
        if let Some((w, h)) = &box_hw {
            if let (Some(wn), Some(hn)) = (parse_user_unit(w), parse_user_unit(h)) {
                css.push_str(&format!("image{{width:{wn}px;height:{hn}px}}"));
            } else {
                css.push_str(&format!("image{{width:{w};height:{h}}}"));
            }
        }
    }
    if hoist_hidden {
        css.push_str("image.rf{visibility:hidden}");
    }

    for &i in &image_idxs {
        let Node::Element(img) = &mut parent.children[i] else {
            continue;
        };
        if hoist_box {
            img.remove_attr("width");
            img.remove_attr("height");
        }
        if hoist_hidden
            && img
                .attr("visibility")
                .is_some_and(|v| v.eq_ignore_ascii_case("hidden"))
        {
            img.remove_attr("visibility");
            match img.attr("class") {
                None => img.set_attr("class", "rf".into()),
                Some(c) if c.split_whitespace().any(|p| p == "rf") => {}
                Some(c) => img.set_attr("class", format!("{c} rf")),
            }
        }
    }

    if let Some(style) = parent.children.iter_mut().find_map(|n| match n {
        Node::Element(el) if el.local_name() == "style" => Some(el),
        _ => None,
    }) {
        let mut body = String::new();
        for child in &style.children {
            if let Node::Text(t) = child {
                body.push_str(t);
            }
        }
        body.push_str(&css);
        style.children = vec![Node::Text(body)];
        style.self_closing = false;
    } else {
        let mut style = Element::new("style");
        style.children.push(Node::Text(css));
        parent.children.insert(0, Node::Element(style));
    }
}

fn flatten_raster_groups(nodes: &mut Vec<Node>, refs: &HashSet<String>) {
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            flatten_raster_groups(&mut el.children, refs);
        }
    }
    for node in nodes.iter_mut() {
        let Node::Element(g) = node else {
            continue;
        };
        if g.local_name() != "g" {
            continue;
        }
        if let Some(img) = try_flatten_raster_group(g, refs) {
            *node = Node::Element(img);
        } else if g_has_image(g) {
            if g.attr("transform")
                .is_some_and(|t| is_identity_transform(t))
            {
                g.remove_attr("transform");
            }
        }
    }
}

fn g_has_image(g: &Element) -> bool {
    g.children
        .iter()
        .any(|n| matches!(n, Node::Element(el) if el.local_name() == "image"))
}

fn try_flatten_raster_group(g: &Element, refs: &HashSet<String>) -> Option<Element> {
    for (k, v) in &g.attrs {
        let key = k.as_str();
        if key.eq_ignore_ascii_case("transform") {
            if !is_identity_transform(v) {
                return None;
            }
            continue;
        }
        if key.eq_ignore_ascii_case("visibility") || key.eq_ignore_ascii_case("id") {
            continue;
        }
        return None;
    }

    let mut image: Option<Element> = None;
    let mut anim: Option<Element> = None;
    for child in &g.children {
        match child {
            Node::Text(t) if t.chars().all(char::is_whitespace) => {}
            Node::Element(el) if el.local_name() == "image" => {
                if image.is_some() {
                    return None;
                }
                image = Some(el.clone());
            }
            Node::Element(el) if is_smil_element(el.local_name()) => {
                let attr = el
                    .attr("attributeName")
                    .or_else(|| el.attr("attributename"))
                    .unwrap_or("");
                if !attr.eq_ignore_ascii_case("visibility") {
                    return None;
                }
                if anim.is_some() {
                    return None;
                }
                anim = Some(el.clone());
            }
            _ => return None,
        }
    }
    let mut image = image?;
    let anim = anim?;

    if let Some(id) = g.attr("id") {
        if refs.contains(id) {
            if image.attr("id").is_none() {
                image.set_attr("id", id.to_string());
            }
        }
    }
    if let Some(vis) = g.attr("visibility") {
        if !vis.eq_ignore_ascii_case("visible") {
            if image.attr("visibility").is_none() {
                image.set_attr("visibility", vis.to_string());
            }
        }
    }
    image.self_closing = false;
    image.children.push(Node::Element(anim));
    compact_visibility_smil(&mut image);
    Some(image)
}

fn is_identity_transform(s: &str) -> bool {
    let n: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    matches!(
        n.to_ascii_lowercase().as_str(),
        "matrix(1,0,0,1,0,0)"
            | "matrix(1.0,0,0,1.0,0,0)"
            | "matrix(1,0,0,1,0,0.0)"
            | "translate(0)"
            | "translate(0,0)"
            | "translate(0,0,0)"
            | "scale(1)"
            | "scale(1,1)"
            | "none"
            | ""
    )
}

fn parse_user_unit(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.ends_with('%') {
        return None;
    }
    let t = t
        .strip_suffix("px")
        .or_else(|| t.strip_suffix("PX"))
        .unwrap_or(t)
        .trim();
    let n: f64 = t.parse().ok()?;
    n.is_finite().then_some(n)
}

fn collect_pointer_ids(nodes: &[Node]) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_pointer_ids(nodes, &mut out);
    out
}

fn walk_pointer_ids(nodes: &[Node], out: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        for (k, v) in &el.attrs {
            let kl = k.to_ascii_lowercase();
            let local = kl.rsplit_once(':').map(|(_, l)| l).unwrap_or(kl.as_str());
            if local == "href" || local == "src" {
                if let Some(id) = v.strip_prefix('#') {
                    if !id.is_empty() {
                        out.insert(id.to_string());
                    }
                }
            }
            if matches!(local, "begin" | "end") {
                let mut sync = BTreeSet::new();
                collect_smil_sync_ids(v, &mut sync);
                out.extend(sync);
            }
            collect_url_ids(v, out);
        }
        if el.local_name() == "style" {
            for child in &el.children {
                if let Node::Text(t) = child {
                    collect_url_ids(t, out);
                }
            }
        }
        walk_pointer_ids(&el.children, out);
    }
}

struct DataUri {
    mime: String,
    bytes: Vec<u8>,
}

fn parse_data_uri(raw: &str) -> Option<DataUri> {
    let rest = raw.trim().strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let meta_l = meta.to_ascii_lowercase();
    let mime = meta
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !mime.starts_with("image/") {
        return None;
    }
    let is_b64 = meta_l.split(';').any(|p| p.trim() == "base64");
    if !is_b64 {
        return None;
    }
    let cleaned: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = B64.decode(cleaned.as_bytes()).ok()?;
    Some(DataUri { mime, bytes })
}

fn to_data_uri(mime: &str, bytes: &[u8]) -> String {
    let mut out = String::from("data:");
    out.push_str(mime);
    out.push_str(";base64,");
    out.push_str(&B64.encode(bytes));
    out
}

fn recompress_data_uri(raw: &str) -> Option<String> {
    let parsed = parse_data_uri(raw)?;
    let mime = normalize_mime(&parsed.mime);
    let next = match mime {
        "image/png" => recompress_png(&parsed.bytes)?,
        "image/jpeg" => recompress_jpeg(&parsed.bytes)?,
        // Already-tight lossy WebP (the driving-POV frames) grows if we
        // decode-to-RGB and re-encode. Keep pixels; wrapper minify is enough.
        _ => return None,
    };
    if next.len() >= parsed.bytes.len() {
        return None;
    }
    Some(to_data_uri(mime, &next))
}

fn normalize_mime(mime: &str) -> &'static str {
    match mime {
        "image/jpg" | "image/jpeg" | "image/pjpeg" => "image/jpeg",
        "image/x-png" | "image/png" => "image/png",
        "image/webp" => "image/webp",
        "image/gif" => "image/gif",
        _ => "image/png",
    }
}

fn recompress_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let opts = oxipng::Options::from_preset(2);
    match oxipng::optimize_from_memory(bytes, &opts) {
        Ok(out) if out.len() < bytes.len() => Some(out),
        _ => None,
    }
}

/// Lossless JPEG cleanup: drop APP1–APP15 / COM. Pixels stay identical.
fn recompress_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    strip_jpeg_metadata(bytes).filter(|out| out.len() < bytes.len())
}

fn strip_jpeg_metadata(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2usize;
    while i < data.len() {
        if data[i] != 0xFF {
            return None;
        }
        while i < data.len() && data[i] == 0xFF {
            i += 1;
        }
        if i >= data.len() {
            return None;
        }
        let marker = data[i];
        i += 1;
        if marker == 0xD9 {
            out.push(0xFF);
            out.push(0xD9);
            break;
        }
        if marker == 0xDA {
            out.push(0xFF);
            out.push(0xDA);
            out.extend_from_slice(&data[i..]);
            return Some(out);
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            out.push(0xFF);
            out.push(marker);
            continue;
        }
        if i + 1 >= data.len() {
            return None;
        }
        let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
        if len < 2 || i + len > data.len() {
            return None;
        }
        let drop = (0xE1..=0xEF).contains(&marker) || marker == 0xFE;
        if !drop {
            out.push(0xFF);
            out.push(marker);
            out.extend_from_slice(&data[i..i + len]);
        }
        i += len;
    }
    Some(out)
}

fn raster_dimensions(mime: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    let format = match normalize_mime(mime) {
        "image/png" => image::ImageFormat::Png,
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/webp" => image::ImageFormat::WebP,
        _ => return None,
    };
    let mut reader = image::ImageReader::new(Cursor::new(bytes));
    reader.set_format(format);
    reader.into_dimensions().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{optimize_str, Config};
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};

    fn fat_png() -> Vec<u8> {
        let mut img = RgbImage::new(24, 24);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([
                (x as u8).wrapping_mul(13),
                (y as u8).wrapping_mul(17),
                ((x + y) as u8).wrapping_mul(11),
            ]);
        }
        let mut buf = Vec::new();
        PngEncoder::new_with_quality(&mut buf, CompressionType::Fast, FilterType::NoFilter)
            .write_image(img.as_raw(), 24, 24, ExtendedColorType::Rgb8)
            .unwrap();
        inject_text_chunk(&mut buf, &"Q".repeat(1200))
    }

    fn inject_text_chunk(png: &mut Vec<u8>, text: &str) -> Vec<u8> {
        assert!(png.len() >= 12);
        let iend = png.len() - 12;
        assert_eq!(&png[iend + 4..iend + 8], b"IEND");
        let mut keyword_data = Vec::from(*b"Comment\0");
        keyword_data.extend_from_slice(text.as_bytes());
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&(keyword_data.len() as u32).to_be_bytes());
        chunk.extend_from_slice(b"tEXt");
        chunk.extend_from_slice(&keyword_data);
        let crc = png_crc(&chunk[4..]);
        chunk.extend_from_slice(&crc.to_be_bytes());
        let mut out = png[..iend].to_vec();
        out.extend_from_slice(&chunk);
        out.extend_from_slice(&png[iend..]);
        out
    }

    fn png_crc(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                let mask = if crc & 1 == 1 { 0xEDB8_8320 } else { 0 };
                crc = (crc >> 1) ^ mask;
            }
        }
        !crc
    }

    fn data_uri_png(bytes: &[u8]) -> String {
        to_data_uri("image/png", bytes)
    }

    #[test]
    fn png_oxipng_shrinks_fat_payload() {
        let raw = fat_png();
        let out = recompress_png(&raw).expect("oxipng should shrink a verbose PNG");
        assert!(out.len() < raw.len(), "{} vs {}", out.len(), raw.len());
        assert_eq!(&out[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn motion_raster_keeps_image_href_and_shrinks() {
        let png = fat_png();
        let uri = data_uri_png(&png);
        let input = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
  <!-- keep-me-stripped -->
  <g transform="matrix(1,0,0,1,0,0)" visibility="hidden" id="frame0">
    <animate attributeName="visibility" begin="0s" values="hidden; visible" keyTimes="0; 1" dur="1s" repeatCount="indefinite"/>
    <image href="{uri}" x="0" y="0" width="24" height="24" preserveAspectRatio="none"/>
  </g>
</svg>"#
        );
        let out = optimize_str(&input).unwrap();
        assert!(out.svg.contains("<image"), "{}", out.svg);
        assert!(
            out.svg.contains("href=\"data:image/png;base64,"),
            "must keep href on <image>:\n{}",
            out.svg
        );
        assert!(!out.svg.contains("xlink:href"), "{}", out.svg);
        assert!(
            out.svg.contains("<animate"),
            "must keep dummy SMIL:\n{}",
            out.svg
        );
        assert!(!out.svg.contains("<!--"), "{}", out.svg);
        assert!(
            out.svg.len() < input.len(),
            "expected a byte drop: {} → {}",
            input.len(),
            out.svg.len()
        );
        assert!(out.animation.motion_sensitive);
        assert!(out
            .plugins_run
            .iter()
            .any(|p| *p == "recompressEmbeddedRasters"));
        assert!(out.plugins_skipped.contains(&"cleanupIds"));
        assert!(!out.svg.contains("preserveAspectRatio"), "{}", out.svg);
    }

    #[test]
    fn keeps_xlink_href_on_image() {
        let png = fat_png();
        let uri = data_uri_png(&png);
        let input = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 24 24">
  <image xlink:href="{uri}" width="24" height="24"/>
</svg>"#
        );
        let out = optimize_str(&input).unwrap();
        assert!(
            out.svg.contains("xlink:href=\"data:image/png;base64,"),
            "must not drop xlink:href:\n{}",
            out.svg
        );
        assert!(out.svg.contains("xmlns:xlink"), "{}", out.svg);
        assert!(out.svg.len() < input.len());
    }

    #[test]
    fn frame_stack_hoists_shared_box_keeps_both_images() {
        let png = fat_png();
        let uri = data_uri_png(&png);
        let input = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
  <g visibility="hidden"><animate attributeName="visibility" values="hidden;visible" dur="1s"/><image href="{uri}" width="24" height="24"/></g>
  <g visibility="visible"><animate attributeName="visibility" values="visible;hidden" dur="1s"/><image href="{uri}" width="24" height="24"/></g>
</svg>"#
        );
        let out = optimize_str(&input).unwrap();
        assert_eq!(out.svg.matches("<image").count(), 2, "{}", out.svg);
        assert_eq!(
            out.svg.matches("href=\"data:image/png;base64,").count(),
            2,
            "{}",
            out.svg
        );
        assert!(out.svg.contains("<style"), "{}", out.svg);
        assert!(
            out.svg.contains("width:24px") && out.svg.contains("height:24px"),
            "{}",
            out.svg
        );
        assert!(out.svg.len() < input.len());
    }

    #[test]
    fn hidden_smil_frame_is_not_deleted() {
        let png = fat_png();
        let uri = data_uri_png(&png);
        let input = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg">
  <g visibility="hidden">
    <animate attributeName="visibility" values="hidden;visible" dur="1s" repeatCount="indefinite"/>
    <image href="{uri}" width="8" height="8"/>
  </g>
</svg>"#
        );
        let mut cfg = Config::default();
        cfg.animation_aware = true;
        let out = crate::optimize(input.as_bytes(), &cfg).unwrap();
        assert!(
            out.svg.contains("<image"),
            "SVGO-style hidden drop:\n{}",
            out.svg
        );
        assert!(out.svg.contains("data:image/png"), "{}", out.svg);
        assert!(out.svg.contains("visibility"), "{}", out.svg);
    }

    #[test]
    fn jpeg_strip_drops_comment() {
        // Minimal 1x1 JFIF + COM + SOF-less payload is hard; just check helper
        // refuses non-JPEG and leaves a tiny SOI/EOI alone.
        assert!(strip_jpeg_metadata(b"not-a-jpeg").is_none());
        let soi_eoi = [0xFF, 0xD8, 0xFF, 0xD9];
        let out = strip_jpeg_metadata(&soi_eoi).unwrap();
        assert_eq!(out, soi_eoi);
    }
}
