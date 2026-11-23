//! TrueType fonts: a glyph-outline reader (`cmap` formats 4 and 12, `glyf` simple and composite
//! glyphs, `hmtx`), a scanline rasterizer with anti-aliasing, and a glyph atlas for ASCII
//! 32..126 at several sizes. Used by the overlay renderer, the software canvas and the README
//! title tool. The bundled face is Inter SemiBold (SIL Open Font License 1.1,
//! `assets/fonts/Inter-LICENSE.txt`).

use std::sync::OnceLock;

/// An outline segment in font units (y up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Close,
}

pub struct Font {
    data: Vec<u8>,
    glyf: usize,
    loca: usize,
    hmtx: usize,
    cmap_sub: usize,
    cmap_fmt: u16,
    long_loca: bool,
    num_hmetrics: usize,
    num_glyphs: usize,
    pub units_per_em: f32,
    pub ascender: f32,
    pub descender: f32,
    pub line_gap: f32,
    pub cap_height: f32,
}

fn u16_at(d: &[u8], o: usize) -> u16 {
    d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]])).unwrap_or(0)
}
fn i16_at(d: &[u8], o: usize) -> i16 {
    u16_at(d, o) as i16
}
fn u32_at(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0)
}

impl Font {
    pub fn parse(data: Vec<u8>) -> Result<Font, String> {
        let n = u16_at(&data, 4) as usize;
        let mut tables = std::collections::HashMap::new();
        for i in 0..n {
            let r = 12 + i * 16;
            let tag = data.get(r..r + 4).ok_or("truncated table directory")?;
            tables.insert(String::from_utf8_lossy(tag).into_owned(), u32_at(&data, r + 8) as usize);
        }
        let t = |name: &str| tables.get(name).copied().ok_or_else(|| format!("no '{name}' table (TrueType outlines needed)"));
        let head = t("head")?;
        let hhea = t("hhea")?;
        let maxp = t("maxp")?;
        let cmap = t("cmap")?;
        // the best Unicode subtable: (3,10) format 12, else (3,1) / (0,x) format 4
        let mut sub = None;
        for i in 0..u16_at(&data, cmap + 2) as usize {
            let r = cmap + 4 + i * 8;
            let (pid, eid, off) = (u16_at(&data, r), u16_at(&data, r + 2), u32_at(&data, r + 4) as usize);
            let fmt = u16_at(&data, cmap + off);
            let score = match (pid, eid, fmt) {
                (3, 10, 12) | (0, _, 12) => 3,
                (3, 1, 4) | (0, _, 4) => 2,
                _ => 0,
            };
            if score > 0 && sub.map(|(s, _, _)| score > s).unwrap_or(true) {
                sub = Some((score, cmap + off, fmt));
            }
        }
        let (_, cmap_sub, cmap_fmt) = sub.ok_or("no Unicode cmap subtable")?;
        let os2 = tables.get("OS/2").copied();
        let units_per_em = u16_at(&data, head + 18) as f32;
        let ascender = i16_at(&data, hhea + 4) as f32;
        let cap = os2.filter(|&o| u16_at(&data, o) >= 2).map(|o| i16_at(&data, o + 88) as f32).filter(|c| *c > 0.0).unwrap_or(ascender * 0.75);
        Ok(Font {
            glyf: t("glyf")?,
            loca: t("loca")?,
            hmtx: t("hmtx")?,
            cmap_sub,
            cmap_fmt,
            long_loca: i16_at(&data, head + 50) == 1,
            num_hmetrics: u16_at(&data, hhea + 34) as usize,
            num_glyphs: u16_at(&data, maxp + 4) as usize,
            units_per_em,
            ascender,
            descender: i16_at(&data, hhea + 6) as f32,
            line_gap: i16_at(&data, hhea + 8) as f32,
            cap_height: cap,
            data,
        })
    }

    pub fn glyph_index(&self, ch: char) -> u16 {
        let c = ch as u32;
        let d = &self.data;
        let s = self.cmap_sub;
        if self.cmap_fmt == 12 {
            let n = u32_at(d, s + 12) as usize;
            for i in 0..n {
                let g = s + 16 + i * 12;
                let (a, b, start) = (u32_at(d, g), u32_at(d, g + 4), u32_at(d, g + 8));
                if c >= a && c <= b {
                    return (start + c - a) as u16;
                }
            }
            return 0;
        }
        if c > 0xFFFF {
            return 0;
        }
        let segs = u16_at(d, s + 6) as usize / 2;
        let ends = s + 14;
        let starts = ends + segs * 2 + 2;
        let deltas = starts + segs * 2;
        let ranges = deltas + segs * 2;
        for i in 0..segs {
            let end = u16_at(d, ends + i * 2) as u32;
            if c > end {
                continue;
            }
            let start = u16_at(d, starts + i * 2) as u32;
            if c < start {
                return 0;
            }
            let delta = u16_at(d, deltas + i * 2);
            let ro = u16_at(d, ranges + i * 2) as usize;
            if ro == 0 {
                return (c as u16).wrapping_add(delta);
            }
            let g = u16_at(d, ranges + i * 2 + ro + (c - start) as usize * 2);
            return if g == 0 { 0 } else { g.wrapping_add(delta) };
        }
        0
    }

    /// Advance width in font units.
    pub fn advance(&self, gid: u16) -> f32 {
        let i = (gid as usize).min(self.num_hmetrics.saturating_sub(1));
        u16_at(&self.data, self.hmtx + i * 4) as f32
    }

    fn glyph_range(&self, gid: u16) -> Option<(usize, usize)> {
        if gid as usize >= self.num_glyphs {
            return None;
        }
        let (a, b) = if self.long_loca {
            (u32_at(&self.data, self.loca + gid as usize * 4) as usize, u32_at(&self.data, self.loca + gid as usize * 4 + 4) as usize)
        } else {
            (u16_at(&self.data, self.loca + gid as usize * 2) as usize * 2, u16_at(&self.data, self.loca + gid as usize * 2 + 2) as usize * 2)
        };
        (b > a).then_some((self.glyf + a, self.glyf + b))
    }

    /// The glyph's outline (font units, y up).
    pub fn outline(&self, gid: u16) -> Vec<Seg> {
        let mut out = Vec::new();
        self.outline_into(gid, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], &mut out, 0);
        out
    }

    fn outline_into(&self, gid: u16, m: [f32; 6], out: &mut Vec<Seg>, depth: u32) {
        let Some((g, _)) = self.glyph_range(gid) else { return };
        let d = &self.data;
        let nc = i16_at(d, g);
        let tf = |x: f32, y: f32| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        if nc >= 0 {
            let nc = nc as usize;
            let mut ends = Vec::with_capacity(nc);
            for i in 0..nc {
                ends.push(u16_at(d, g + 10 + i * 2) as usize);
            }
            let npts = ends.last().map(|e| e + 1).unwrap_or(0);
            let ins = u16_at(d, g + 10 + nc * 2) as usize;
            let mut p = g + 12 + nc * 2 + ins;
            let mut flags = Vec::with_capacity(npts);
            while flags.len() < npts {
                let f = d.get(p).copied().unwrap_or(0);
                p += 1;
                flags.push(f);
                if f & 8 != 0 {
                    let r = d.get(p).copied().unwrap_or(0);
                    p += 1;
                    for _ in 0..r {
                        flags.push(f);
                    }
                }
            }
            flags.truncate(npts);
            let mut coords = |short: u8, same: u8| -> Vec<f32> {
                let mut v = Vec::with_capacity(npts);
                let mut c = 0i32;
                for &f in &flags {
                    if f & short != 0 {
                        let b = d.get(p).copied().unwrap_or(0) as i32;
                        p += 1;
                        c += if f & same != 0 { b } else { -b };
                    } else if f & same == 0 {
                        c += i16_at(d, p) as i32;
                        p += 2;
                    }
                    v.push(c as f32);
                }
                v
            };
            let xs = coords(2, 16);
            let ys = coords(4, 32);
            let mut st = 0;
            for &e in &ends {
                let pts: Vec<(f32, f32, bool)> = (st..=e).map(|i| (xs[i], ys[i], flags[i] & 1 != 0)).collect();
                st = e + 1;
                if pts.is_empty() {
                    continue;
                }
                // start at an on-curve point (or the midpoint of two off-curve points)
                let n = pts.len();
                let first_on = pts.iter().position(|q| q.2);
                let (sx, sy, from) = match first_on {
                    Some(k) => (pts[k].0, pts[k].1, k),
                    None => ((pts[0].0 + pts[n - 1].0) / 2.0, (pts[0].1 + pts[n - 1].1) / 2.0, n - 1),
                };
                let (tx, ty) = tf(sx, sy);
                out.push(Seg::Move(tx, ty));
                let mut ctrl: Option<(f32, f32)> = None;
                for k in 1..=n {
                    let q = pts[(from + k) % n];
                    if q.2 {
                        let (x, y) = tf(q.0, q.1);
                        match ctrl.take() {
                            Some((cx, cy)) => out.push(Seg::Quad(cx, cy, x, y)),
                            None => out.push(Seg::Line(x, y)),
                        }
                    } else {
                        let (qx, qy) = tf(q.0, q.1);
                        if let Some((cx, cy)) = ctrl {
                            out.push(Seg::Quad(cx, cy, (cx + qx) / 2.0, (cy + qy) / 2.0));
                        }
                        ctrl = Some((qx, qy));
                    }
                }
                if let Some((cx, cy)) = ctrl {
                    let (x, y) = tf(sx, sy);
                    out.push(Seg::Quad(cx, cy, x, y));
                }
                out.push(Seg::Close);
            }
        } else if depth < 8 {
            // composite
            let mut p = g + 10;
            loop {
                let flags = u16_at(d, p);
                let child = u16_at(d, p + 2);
                p += 4;
                let (dx, dy) = if flags & 1 != 0 {
                    let r = (i16_at(d, p) as f32, i16_at(d, p + 2) as f32);
                    p += 4;
                    r
                } else {
                    let r = (d.get(p).copied().unwrap_or(0) as i8 as f32, d.get(p + 1).copied().unwrap_or(0) as i8 as f32);
                    p += 2;
                    r
                };
                let f2 = |o: usize| i16_at(d, o) as f32 / 16384.0;
                let (mut a, mut b, mut c, mut e) = (1.0, 0.0, 0.0, 1.0);
                if flags & 8 != 0 {
                    a = f2(p);
                    e = a;
                    p += 2;
                } else if flags & 0x40 != 0 {
                    a = f2(p);
                    e = f2(p + 2);
                    p += 4;
                } else if flags & 0x80 != 0 {
                    a = f2(p);
                    b = f2(p + 2);
                    c = f2(p + 4);
                    e = f2(p + 6);
                    p += 8;
                }
                // child transform, then the parent's
                let cm = [a, b, c, e, dx, dy];
                let mm = [m[0] * cm[0] + m[2] * cm[1], m[1] * cm[0] + m[3] * cm[1], m[0] * cm[2] + m[2] * cm[3], m[1] * cm[2] + m[3] * cm[3], m[0] * cm[4] + m[2] * cm[5] + m[4], m[1] * cm[4] + m[3] * cm[5] + m[5]];
                if flags & 2 != 0 {
                    self.outline_into(child, mm, out, depth + 1);
                }
                if flags & 0x20 == 0 {
                    break;
                }
            }
        }
    }
}

/// Rasterizes an outline (font units, y up) at `scale` pixels per unit with the origin (pen
/// position on the baseline) at (`ox`, `oy`) in a `w`×`h` coverage bitmap (y down).
pub fn rasterize(segs: &[Seg], scale: f32, ox: f32, oy: f32, w: usize, h: usize) -> Vec<u8> {
    // flatten to edges in pixel space
    let mut edges: Vec<(f32, f32, f32, f32)> = Vec::new();
    let (mut cx, mut cy, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
    let px = |x: f32, y: f32| (ox + x * scale, oy - y * scale);
    for s in segs {
        match *s {
            Seg::Move(x, y) => {
                let p = px(x, y);
                (cx, cy, sx, sy) = (p.0, p.1, p.0, p.1);
            }
            Seg::Line(x, y) => {
                let p = px(x, y);
                edges.push((cx, cy, p.0, p.1));
                (cx, cy) = p;
            }
            Seg::Quad(qx, qy, x, y) => {
                let c = px(qx, qy);
                let p = px(x, y);
                let n = (((c.0 - cx).abs() + (c.1 - cy).abs() + (p.0 - c.0).abs() + (p.1 - c.1).abs()) / 2.0).ceil().clamp(2.0, 32.0) as usize;
                let (mut lx, mut ly) = (cx, cy);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    let x2 = u * u * cx + 2.0 * u * t * c.0 + t * t * p.0;
                    let y2 = u * u * cy + 2.0 * u * t * c.1 + t * t * p.1;
                    edges.push((lx, ly, x2, y2));
                    (lx, ly) = (x2, y2);
                }
                (cx, cy) = p;
            }
            Seg::Close => {
                if (cx, cy) != (sx, sy) {
                    edges.push((cx, cy, sx, sy));
                }
                (cx, cy) = (sx, sy);
            }
        }
    }
    let mut acc = vec![0f32; w * h];
    const SUB: usize = 8;
    let mut xs: Vec<(f32, i32)> = Vec::new();
    for row in 0..h {
        for s in 0..SUB {
            let y = row as f32 + (s as f32 + 0.5) / SUB as f32;
            xs.clear();
            for &(x0, y0, x1, y1) in &edges {
                if (y0 <= y && y1 > y) || (y1 <= y && y0 > y) {
                    let x = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                    xs.push((x, if y1 > y0 { 1 } else { -1 }));
                }
            }
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut wind = 0;
            for k in 0..xs.len() {
                let before = wind;
                wind += xs[k].1;
                if before == 0 && wind != 0 {
                    // span starts; find its end
                    let start = xs[k].0;
                    let mut w2 = wind;
                    let mut end = start;
                    for j in k + 1..xs.len() {
                        w2 += xs[j].1;
                        if w2 == 0 {
                            end = xs[j].0;
                            break;
                        }
                    }
                    let (a, b) = (start.max(0.0), end.min(w as f32));
                    if b > a {
                        let (ia, ib) = (a.floor() as usize, (b.ceil() as usize).min(w));
                        for xp in ia..ib {
                            let cov = (b.min(xp as f32 + 1.0) - a.max(xp as f32)).max(0.0);
                            acc[row * w + xp] += cov / SUB as f32;
                        }
                    }
                }
            }
        }
    }
    acc.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect()
}

/// One glyph in an atlas.
#[derive(Debug, Clone, Copy, Default)]
pub struct Glyph {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// bitmap offset from the pen position at the cap line (pixels)
    pub left: f32,
    pub top: f32,
    pub advance: f32,
}

/// One size of the face in an atlas.
#[derive(Debug, Clone)]
pub struct Face {
    pub px: f32,
    pub cap: f32,
    pub glyphs: Vec<Glyph>,
}

/// An R8 coverage atlas with faces for several sizes and a solid block (for rectangles).
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub faces: Vec<Face>,
    pub solid: (u32, u32),
}

/// Pixel size of the text at a menu scale (scale 2 = 17 px).
pub fn px_for_scale(scale: f32) -> f32 {
    (8.5 * scale.max(0.5)).round()
}

impl Atlas {
    pub fn build(font: &Font, sizes: &[f32]) -> Atlas {
        let width = 1024u32;
        let mut pixels = vec![0u8; width as usize * 8];
        // solid 4x4 block at (0, 0)
        for y in 0..4 {
            for x in 0..4 {
                pixels[y * width as usize + x] = 255;
            }
        }
        let (mut pen_x, mut pen_y, mut row_h) = (6u32, 0u32, 6u32);
        let mut faces = Vec::new();
        for &px in sizes {
            let scale = px / font.units_per_em;
            let cap = font.cap_height * scale;
            let mut glyphs = Vec::with_capacity(95);
            for code in 32u8..127 {
                let ch = code as char;
                let gid = font.glyph_index(ch);
                let adv = font.advance(gid) * scale;
                let segs = font.outline(gid);
                if segs.is_empty() {
                    glyphs.push(Glyph { advance: adv, ..Default::default() });
                    continue;
                }
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for s in &segs {
                    let pts: &[(f32, f32)] = &match *s {
                        Seg::Move(x, y) | Seg::Line(x, y) => [(x, y), (x, y)],
                        Seg::Quad(a, b, x, y) => [(a, b), (x, y)],
                        Seg::Close => continue,
                    };
                    for &(x, y) in pts {
                        x0 = x0.min(x);
                        x1 = x1.max(x);
                        y0 = y0.min(y);
                        y1 = y1.max(y);
                    }
                }
                let left = (x0 * scale).floor();
                let top = (y1 * scale).ceil();
                let gw = ((x1 * scale).ceil() - left) as u32 + 1;
                let gh = (top - (y0 * scale).floor()) as u32 + 1;
                if pen_x + gw + 1 > width {
                    pen_x = 0;
                    pen_y += row_h + 1;
                    row_h = 0;
                }
                let need = ((pen_y + gh + 1) * width) as usize;
                if pixels.len() < need {
                    pixels.resize(need, 0);
                }
                let bmp = rasterize(&segs, scale, -left, top, gw as usize, gh as usize);
                for y in 0..gh as usize {
                    let o = (pen_y as usize + y) * width as usize + pen_x as usize;
                    pixels[o..o + gw as usize].copy_from_slice(&bmp[y * gw as usize..(y + 1) * gw as usize]);
                }
                glyphs.push(Glyph { x: pen_x, y: pen_y, w: gw, h: gh, left, top: cap - top, advance: adv });
                pen_x += gw + 1;
                row_h = row_h.max(gh);
            }
            faces.push(Face { px, cap, glyphs });
        }
        let height = (pen_y + row_h + 1).next_power_of_two();
        pixels.resize((width * height) as usize, 0);
        Atlas { width, height, pixels, faces, solid: (1, 1) }
    }

    /// The face closest to `px`.
    pub fn face(&self, px: f32) -> &Face {
        self.faces.iter().min_by(|a, b| (a.px - px).abs().partial_cmp(&(b.px - px).abs()).unwrap()).unwrap()
    }

    pub fn glyph(&self, face: &Face, ch: char) -> Glyph {
        let c = ch as u32;
        let i = if (32..127).contains(&c) { (c - 32) as usize } else { ('?' as u32 - 32) as usize };
        face.glyphs[i]
    }

    pub fn text_width(&self, s: &str, scale: f32) -> f32 {
        let f = self.face(px_for_scale(scale));
        s.chars().map(|c| self.glyph(f, c).advance).sum()
    }
}

static INTER: OnceLock<Font> = OnceLock::new();
static ATLAS: OnceLock<Atlas> = OnceLock::new();

/// Inter SemiBold (bundled).
pub fn inter() -> &'static Font {
    INTER.get_or_init(|| Font::parse(include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf").to_vec()).expect("bundled font"))
}

/// The shared atlas: Inter at the sizes of scales 1, 1.5, 2, 2.5, 3, 4.
pub fn atlas() -> &'static Atlas {
    ATLAS.get_or_init(|| Atlas::build(inter(), &[1.0f32, 1.5, 2.0, 2.5, 3.0, 4.0].map(px_for_scale)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inter_parses_and_rasterizes() {
        let f = inter();
        assert_eq!(f.units_per_em, 2048.0);
        let a = f.glyph_index('A');
        assert!(a > 0);
        assert!(f.advance(a) > 1000.0);
        let o = f.outline(a);
        assert!(o.iter().filter(|s| matches!(s, Seg::Move(..))).count() >= 2, "A has an outer and an inner contour");
        // 'i' (possibly composite) has two parts
        assert!(f.outline(f.glyph_index('i')).iter().filter(|s| matches!(s, Seg::Move(..))).count() >= 2);
        let at = atlas();
        let face = at.face(17.0);
        let g = at.glyph(face, 'H');
        assert!(g.w > 6 && g.h >= 11 && g.h <= 15, "{g:?}");
        // the middle of H's left stem is solid, the middle of the counter is empty
        let px = |x: u32, y: u32| at.pixels[((g.y + y) * at.width + g.x + x) as usize];
        assert!(px(1, 2) > 200 || px(2, 2) > 200, "stem");
        assert_eq!(px(g.w / 2, 2), 0, "counter");
        assert!(at.text_width("Hello", 2.0) > at.text_width("Hi", 2.0));
        // 'l' is narrower than 'm'
        assert!(at.glyph(face, 'l').advance < at.glyph(face, 'm').advance);
    }
}
