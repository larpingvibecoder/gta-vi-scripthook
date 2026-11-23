//! `shk-title`: draws the README title as SVG paths (text converted to outlines, so it looks the
//! same everywhere) with the repository's own TrueType reader (`scripthook_sdk::font`).
//!
//! `shk-title <font.ttf> <out dir>` writes `title-dark.svg` (for dark pages: light text) and
//! `title-light.svg` (for light pages: dark text).

use scripthook_sdk::font::{Font, Seg};

fn path_of(font: &Font, text: &str, size: f32, x0: f32, baseline: f32, tracking: f32) -> (String, f32) {
    let s = size / font.units_per_em;
    let mut d = String::new();
    let mut pen = x0;
    let f = |v: f32| {
        let r = (v * 100.0).round() / 100.0;
        if r.fract() == 0.0 {
            format!("{}", r as i64)
        } else {
            format!("{r}")
        }
    };
    for ch in text.chars() {
        let g = font.glyph_index(ch);
        for seg in font.outline(g) {
            match seg {
                Seg::Move(x, y) => d.push_str(&format!("M{} {}", f(pen + x * s), f(baseline - y * s))),
                Seg::Line(x, y) => d.push_str(&format!("L{} {}", f(pen + x * s), f(baseline - y * s))),
                Seg::Quad(cx, cy, x, y) => d.push_str(&format!("Q{} {} {} {}", f(pen + cx * s), f(baseline - cy * s), f(pen + x * s), f(baseline - y * s))),
                Seg::Close => d.push('Z'),
            }
        }
        pen += font.advance(g) * s + tracking;
    }
    (d, pen - x0 - tracking)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let font_path = args.first().cloned().unwrap_or_else(|| "assets/fonts/InterDisplay-Bold.ttf".into());
    let out = std::path::PathBuf::from(args.get(1).cloned().unwrap_or_else(|| "assets".into()));
    let font = Font::parse(std::fs::read(&font_path).expect("font file")).expect("TrueType font");
    let size = 88.0;
    let tracking = -1.5;
    let pad = 8.0;
    let s = size / font.units_per_em;
    let asc = font.ascender * s;
    let desc = -font.descender * s;
    let (_, wa) = path_of(&font, "gta-vi-", size, 0.0, 0.0, tracking);
    let (_, wb) = path_of(&font, "scripthook", size, 0.0, 0.0, tracking);
    let width = (pad * 2.0 + wa + tracking + wb).ceil();
    let height = (asc + desc + pad * 3.0).ceil();
    let baseline = (asc + pad).round();
    let (pa, _) = path_of(&font, "gta-vi-", size, pad, baseline, tracking);
    let (pb, _) = path_of(&font, "scripthook", size, pad + wa + tracking, baseline, tracking);
    for (name, muted, strong) in [("title-dark.svg", "#8b98a5", "#f0f6fc"), ("title-light.svg", "#6e7781", "#1f2328")] {
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-label=\"gta-vi-scripthook\">\n  <title>gta-vi-scripthook</title>\n  <!-- text drawn as outlines from Inter Display Bold (SIL Open Font License 1.1) by tools/title -->\n  <path fill=\"{muted}\" d=\"{pa}\"/>\n  <path fill=\"{strong}\" d=\"{pb}\"/>\n</svg>\n"
        );
        let p = out.join(name);
        std::fs::write(&p, svg).expect("write");
        println!("wrote {} ({width}x{height})", p.display());
    }
    // a PNG preview of the same outlines (optional third argument), rasterized by the SDK
    if let Some(png) = args.get(2) {
        let (w, h) = (width as usize, height as usize);
        let mut rgba = vec![0u8; w * h * 4];
        for px in rgba.chunks_mut(4) {
            px.copy_from_slice(&[13, 17, 23, 255]);
        }
        let mut pen = pad;
        for (i, ch) in "gta-vi-scripthook".chars().enumerate() {
            let g = font.glyph_index(ch);
            let cov = scripthook_sdk::font::rasterize(&font.outline(g), s, pen, baseline, w, h);
            let col: [u8; 3] = if i < 7 { [139, 152, 165] } else { [240, 246, 252] };
            for (k, c) in cov.iter().enumerate() {
                if *c > 0 {
                    let a = *c as u32;
                    for j in 0..3 {
                        let d = rgba[k * 4 + j] as u32;
                        rgba[k * 4 + j] = ((col[j] as u32 * a + d * (255 - a)) / 255) as u8;
                    }
                }
            }
            pen += font.advance(g) * s + tracking;
        }
        std::fs::write(png, scripthook_sdk::png::encode_rgba(w as u32, h as u32, &rgba)).expect("write png");
    }
}
