//! Visual-fidelity guards for convertPathData / mergePaths.
//!
//! The world-map ocean in `corpus/large/blankmap-world-flattened.svg` is a
//! rectangle of axis-aligned cubics (`fill:white`). A bad implicit-command join
//! glued `0` onto the next number (`6.408 014.1912`), which browsers paint as a
//! huge white triangle. Keep the fixture tiny so CI is not 1.7MB-bound.

use minsvg::{minify_path_d, optimize_str};

/// Rectangle of cubics — same construction as the blankmap ocean, ~200 bytes.
const OCEAN_RECT: &str = "M 100,200 C 80,200 60,200 40,200 C 40,160 40,120 40,80 C 60,80 80,80 100,80 C 100,120 100,160 100,200";

/// Two-cubic top-edge excerpt that used to emit `6.408 014.1912`.
const OCEAN_TOP_EDGE: &str = "M -26.0058,61.9866 C -23.8711,61.9866 -21.7344,61.9866 -19.5978,61.9866 C -5.4066,61.9866 8.7864,61.9866 22.9794,61.9866";

fn extract_d<'a>(svg: &'a str, class: &str) -> &'a str {
    let marker = format!("class=\"{class}\"");
    let start = svg
        .find(&marker)
        .unwrap_or_else(|| panic!("missing class={class} in {svg}"));
    let after = &svg[start..];
    let d0 = after
        .find("d=\"")
        .unwrap_or_else(|| panic!("missing d after {class}: {svg}"));
    let rest = &after[d0 + 3..];
    let d1 = rest
        .find('"')
        .unwrap_or_else(|| panic!("unterminated d after {class}"));
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
