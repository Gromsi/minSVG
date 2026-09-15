//! Visual-fidelity guards for convertPathData / mergePaths.
//!
//! The world-map ocean in `corpus/large/blankmap-world-flattened.svg` is a
//! rectangle of axis-aligned cubics (`fill:white`). A bad implicit-command join
//! glued `0` onto the next number (`6.408 014.1912`), which browsers paint as a
//! huge white triangle. Keep the fixture tiny so CI is not 1.7MB-bound.

use minsvg::{minify_path_d, minify_path_d_with, optimize_str, optimize_str_with, Config};

/// Rectangle of cubics — same construction as the blankmap ocean, ~200 bytes.
const OCEAN_RECT: &str = "M 100,200 C 80,200 60,200 40,200 C 40,160 40,120 40,80 C 60,80 80,80 100,80 C 100,120 100,160 100,200";

/// Two-cubic top-edge excerpt that used to emit `6.408 014.1912`.
const OCEAN_TOP_EDGE: &str = "M -26.0058,61.9866 C -23.8711,61.9866 -21.7344,61.9866 -19.5978,61.9866 C -5.4066,61.9866 8.7864,61.9866 22.9794,61.9866";

fn extract_d<'a>(svg: &'a str, class: &str) -> &'a str {
    let marker = format!("class=\"{class}\"");
    let start = svg
        .find(&marker)
        .unwrap_or_else(|| panic!("missing class={class} in {svg}"));
    let tag_start = svg[..start].rfind('<').unwrap_or(0);
    let tag = &svg[tag_start..];
    let tag_end = tag.find('>').unwrap_or(tag.len());
    let tag = &tag[..tag_end];
    let d0 = tag
        .find("d=\"")
        .unwrap_or_else(|| panic!("missing d on {class}: {svg}"));
    let rest = &tag[d0 + 3..];
    let d1 = rest
        .find('"')
        .unwrap_or_else(|| panic!("unterminated d on {class}"));
    &rest[..d1]
}

fn count_dest_points(d: &str) -> usize {
    // 1 pair per M/L/T, 1 for H/V, 3 for C, 2 for S/Q, 1 dest + flags for A, 0 for Z.
    // Implicit repeats keep the last command. Good enough to reject a 2–3 point wreck.
    let mut cmd = None;
    let mut nums = 0usize;
    let mut dests = 0usize;
    let flush = |cmd: Option<char>, nums: usize, dests: &mut usize| {
        let Some(c) = cmd else { return };
        let arity = match c {
            'M' | 'm' | 'L' | 'l' | 'T' | 't' => 2,
            'H' | 'h' | 'V' | 'v' => 1,
            'C' | 'c' => 6,
            'S' | 's' | 'Q' | 'q' => 4,
            'A' | 'a' => 7,
            'Z' | 'z' => 0,
            _ => return,
        };
        if arity == 0 {
            return;
        }
        *dests += nums / arity;
    };
    let bytes = d.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_alphabetic() {
            flush(cmd, nums, &mut dests);
            cmd = Some(c);
            nums = 0;
            i += 1;
            continue;
        }
        if c == ',' || c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // number
        if c == '+' || c == '-' || c == '.' || c.is_ascii_digit() {
            nums += 1;
            if c == '+' || c == '-' {
                i += 1;
            }
            let mut saw_dot = false;
            while i < bytes.len() {
                let ch = bytes[i];
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == b'.' && !saw_dot {
                    saw_dot = true;
                    i += 1;
                } else if ch == b'e' || ch == b'E' {
                    i += 1;
                    if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    break;
                } else {
                    break;
                }
            }
            continue;
        }
        i += 1;
    }
    flush(cmd, nums, &mut dests);
    dests
}

#[test]
fn world_map_ocean_cubics_do_not_collapse_to_triangle() {
    let input = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-30 60 2750 1400" width="2750" height="1400">
<style type="text/css">.ocean{{fill:#fff;stroke:#000;stroke-width:.5}}.land{{fill:#b9b9b9;stroke:#fff}}</style>
<path class="ocean" id="ocean" d="{OCEAN_RECT}"/>
<path class="land pe" id="pe" d="M 648.6568,798.4085 C 649.7424,796.3496 650.3516,794.9406 651.9945,793.1866 C 648.3827,800.3356 649.0196,799.6756 648.6568,798.4085"/>
</svg>"##
    );

    let out = optimize_str(&input).unwrap();
    assert!(out.svg.contains("viewBox="), "{}", out.svg);
    assert!(
        out.svg.contains(r#"viewBox="-30 60 2750 1400""#)
            || out.svg.contains("viewBox=\"-30 60 2750 1400\""),
        "viewBox drifted: {}",
        out.svg
    );

    let ocean = extract_d(&out.svg, "ocean");
    assert!(
        !ocean.contains("014.1912") && !ocean.contains("6.4080"),
        "implicit-command 0-glue in ocean: {ocean}"
    );
    let dests = count_dest_points(ocean);
    assert!(
        dests >= 4,
        "ocean collapsed to {dests} dest points (triangle): {ocean}"
    );
    assert!(
        ocean.len() > 20,
        "ocean d too short to be a rectangle: {ocean}"
    );

    let land = extract_d(&out.svg, "land pe");
    assert!(count_dest_points(land) >= 2, "land path vanished: {land}");
}

#[test]
fn capybara_hat_lines_do_not_shoot_off_canvas() {
    let input = concat!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 200 160\">",
        "<line x1=\"139\" y1=\"56\" x2=\"124\" y2=\"42\" stroke=\"#ffb040\" stroke-width=\"1\"/>",
        "<line x1=\"139\" y1=\"56\" x2=\"139\" y2=\"36\" stroke=\"#ffb040\" stroke-width=\"1\"/>",
        "<line x1=\"139\" y1=\"56\" x2=\"154\" y2=\"40\" stroke=\"#ffb040\" stroke-width=\"1\"/>",
        "</svg>"
    );
    let out = optimize_str(input).unwrap();
    assert!(
        !out.svg.contains("M139 56 15-16") && !out.svg.contains("M139 56 15 -16"),
        "hat tick became L(15,-16): {}",
        out.svg
    );
    assert!(
        out.svg.contains("154") || out.svg.contains("x2=\"154\"") || out.svg.contains("l15-16"),
        "right hat tick lost: {}",
        out.svg
    );
}

#[test]
fn ocean_top_edge_minify_keeps_both_cubics() {
    let out = minify_path_d(OCEAN_TOP_EDGE).expect("top-edge snippet");
    assert!(!out.contains("014.1912"), "{out}");
    assert_eq!(count_dest_points(OCEAN_TOP_EDGE), 3);
    assert_eq!(count_dest_points(&out), 3, "lost a cubic dest: {out}");
}

#[test]
fn precision_2_vs_3_size_and_dest_count() {
    const D: &str = "M 1.23456 2.34567 C 3.45678 4.56789 5.67891 6.78901 7.89012 8.90123 C 1.11111 2.22222 3.33333 4.44444 5.55555 6.66666";
    let input = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0.123456 1.234567 10.345678 11.456789"><path class="p" d="{D}"/></svg>"#
    );
    let mut c2 = Config::default();
    c2.precision = Some(2);
    let mut c3 = Config::default();
    c3.precision = Some(3);
    let a = optimize_str_with(&input, &c2).expect("prec 2");
    let b = optimize_str_with(&input, &c3).expect("prec 3");
    assert!(
        !a.svg.contains("014.1912") && !b.svg.contains("014.1912"),
        "digit glue:\n{}\n{}",
        a.svg,
        b.svg
    );
    let da = extract_d(&a.svg, "p");
    let db = extract_d(&b.svg, "p");
    let orig = count_dest_points(D);
    assert_eq!(count_dest_points(da), orig, "prec2 dests: {da}");
    assert_eq!(count_dest_points(db), orig, "prec3 dests: {db}");
    assert!(
        a.svg.len() <= b.svg.len(),
        "prec2 {}B > prec3 {}B\n{}\n{}",
        a.svg.len(),
        b.svg.len(),
        a.svg,
        b.svg
    );
    assert!(
        a.svg.len() < b.svg.len(),
        "expected prec2 smaller: {} vs {}",
        a.svg,
        b.svg
    );

    let ocean2 = minify_path_d_with(OCEAN_TOP_EDGE, Some(2)).expect("ocean prec2");
    let ocean3 = minify_path_d_with(OCEAN_TOP_EDGE, Some(3)).expect("ocean prec3");
    assert!(!ocean2.contains("014.1912") && !ocean3.contains("014.1912"));
    assert_eq!(count_dest_points(&ocean2), 3, "{ocean2}");
    assert_eq!(count_dest_points(&ocean3), 3, "{ocean3}");
}

#[test]
fn ocean_group_transform_does_not_glue_zero_or_bake_d() {
    let input = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-30 60 2750 1400">
<g transform="translate(0.000, 0.000) scale(1.000, 1.000)">
<path class="ocean" id="ocean" d="{OCEAN_RECT}"/>
</g>
</svg>"##
    );
    let out = optimize_str(&input).unwrap();
    let ocean = extract_d(&out.svg, "ocean");
    assert!(
        !ocean.contains("014.1912") && !ocean.contains("6.4080"),
        "implicit-command 0-glue in ocean: {ocean}"
    );
    let dests = count_dest_points(ocean);
    assert!(
        dests >= 4,
        "ocean collapsed to {dests} dest points (triangle): {ocean}"
    );
    assert!(
        !out.svg.contains("transform="),
        "identity translate/scale should drop: {}",
        out.svg
    );
}
