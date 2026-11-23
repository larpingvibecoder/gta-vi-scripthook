//! Drawing the menu: header bar with the project name, breadcrumb bar, rows with a selection
//! highlight, switches, slider tracks, scroll bar, footer key hints, a description panel, toasts
//! and HUD lines. Only rectangles and the host's bitmap font ([`Painter`]).

use crate::nav::{trim_num, Navigator, Row};
use scripthook_sdk::draw::{argb, Painter};
use scripthook_sdk::menu::Kind;

/// Colours (0xAARRGGBB) and metrics.
#[derive(Debug, Clone)]
pub struct Theme {
    pub panel: u32,
    pub header: u32,
    pub subheader: u32,
    pub footer: u32,
    pub accent: u32,
    pub accent_dim: u32,
    pub selection: u32,
    pub text: u32,
    pub muted: u32,
    pub disabled: u32,
    pub warn: u32,
    pub track: u32,
    pub knob: u32,
    pub on: u32,
    pub shadow: u32,
    /// panel width in font pixels
    pub width: f32,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            panel: argb(236, 12, 16, 22),
            header: argb(248, 17, 23, 31),
            subheader: argb(244, 22, 29, 39),
            footer: argb(244, 15, 20, 27),
            accent: argb(255, 46, 196, 182),
            accent_dim: argb(255, 28, 110, 104),
            selection: argb(235, 24, 58, 63),
            text: argb(255, 232, 237, 242),
            muted: argb(255, 138, 150, 163),
            disabled: argb(255, 88, 97, 108),
            warn: argb(255, 242, 184, 75),
            track: argb(255, 44, 52, 62),
            knob: argb(255, 245, 248, 250),
            on: argb(255, 46, 196, 182),
            shadow: argb(90, 0, 0, 0),
            width: 262.0,
        }
    }
}

/// What the renderer needs beyond the rows.
pub struct View<'a> {
    pub title: &'a str,
    pub version: &'a str,
    pub breadcrumb: String,
    pub subtitle: String,
    pub filterable: bool,
    pub hints: &'a [(&'a str, &'a str)],
    pub right_side: bool,
    pub scale: f32,
    pub time: f64,
}

/// A rectangle with its corners cut by `r` (a cheap rounded look with rectangles only).
pub fn rrect(p: &mut dyn Painter, x: f32, y: f32, w: f32, h: f32, r: f32, c: u32) {
    if w <= 2.0 * r || h <= 2.0 * r {
        p.rect(x, y, w, h, c);
        return;
    }
    p.rect(x + r, y, w - 2.0 * r, h, c);
    p.rect(x, y + r, r, h - 2.0 * r, c);
    p.rect(x + w - r, y + r, r, h - 2.0 * r, c);
}

fn clip_text(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        t.push_str("...");
        t
    }
}

/// "A / B / C" shortened from the left ("... / C") to `max` characters.
fn tail_crumb(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let parts: Vec<&str> = s.split(" / ").collect();
    for k in 1..parts.len() {
        let t = format!("... / {}", parts[k..].join(" / "));
        if t.chars().count() <= max {
            return t;
        }
    }
    clip_text(parts.last().copied().unwrap_or(s), max)
}

/// Word-wraps to lines of at most `n` characters, at most `max` lines.
pub fn wrap(s: &str, n: usize, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in s.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > n {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() > max {
        let rest = lines[max - 1..].join(" ");
        lines.truncate(max - 1);
        lines.push(clip_text(&rest, n));
    }
    lines.into_iter().map(|l| clip_text(&l, n)).collect()
}

/// Layout numbers for a scale and screen height.
pub struct Layout {
    pub u: f32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub header_h: f32,
    pub sub_h: f32,
    pub row_h: f32,
    pub footer_h: f32,
    pub visible: usize,
}

pub fn layout(p: &dyn Painter, theme: &Theme, scale: f32, right: bool, filterable: bool) -> Layout {
    let u = scale.max(1.0);
    let (sw, sh) = p.size();
    let w = (theme.width * u).min(sw as f32 - 16.0 * u);
    let header_h = 22.0 * u;
    let sub_h = 14.0 * u;
    let row_h = 13.0 * u;
    let footer_h = 13.0 * u;
    let filter_h = if filterable { row_h } else { 0.0 };
    let margin = 12.0 * u;
    // room for the description panel (about 4 lines) below
    let avail = sh as f32 - 2.0 * margin - header_h - sub_h - footer_h - filter_h - 52.0 * u;
    let visible = ((avail / row_h).floor() as usize).clamp(4, 16);
    let x = if right { sw as f32 - margin - w } else { margin };
    Layout { u, x, y: margin, w, header_h, sub_h, row_h, footer_h, visible }
}

/// Draws the open menu; returns the number of rows that fit (for paging).
pub fn draw_menu(p: &mut dyn Painter, theme: &Theme, view: &View, nav: &Navigator, rows: &[Row]) -> usize {
    let l = layout(p, theme, view.scale, view.right_side, view.filterable);
    let (u, x, w) = (l.u, l.x, l.w);
    let s = u; // text scale
    let adv = (p.text_width("abcdefghijklmnopqrstuvwxyz", s) / 26.0).max(1.0);
    let gh = p.glyph_height(s);
    let cols = ((w - 24.0 * u) / adv) as usize;
    let mut y = l.y;

    // shadow
    let body_rows = rows.len().min(l.visible).max(1);
    let filter_h = if view.filterable { l.row_h } else { 0.0 };
    let total_h = l.header_h + l.sub_h + filter_h + body_rows as f32 * l.row_h + 4.0 * u + l.footer_h;
    rrect(p, x + 2.0 * u, y + 3.0 * u, w, total_h, 2.0 * u, theme.shadow);

    // header: accent strip, project name, version
    rrect(p, x, y, w, l.header_h, 2.0 * u, theme.header);
    p.rect(x, y + l.header_h - u, w, u, theme.accent);
    // the mark: a small hook shape made of blocks
    let mx = x + 10.0 * u;
    let my = y + (l.header_h - 10.0 * u) / 2.0;
    p.rect(mx + 4.0 * u, my, 2.0 * u, 7.0 * u, theme.accent);
    p.rect(mx, my + 7.0 * u, 6.0 * u, 2.0 * u, theme.accent);
    p.rect(mx, my + 4.0 * u, 2.0 * u, 3.0 * u, theme.accent);
    let tx = mx + 12.0 * u;
    let ty = y + (l.header_h - gh) / 2.0 - u * 0.5;
    let (a, b) = view.title.split_at(view.title.find("scripthook").unwrap_or(0));
    let wa = p.text(tx, ty, s, theme.muted, a);
    p.text(tx + wa, ty, s, theme.text, b);
    let vw = p.text_width(view.version, s);
    p.text(x + w - 10.0 * u - vw, ty, s, theme.muted, view.version);
    y += l.header_h;

    // breadcrumb bar
    p.rect(x, y, w, l.sub_h, theme.subheader);
    let crumb = tail_crumb(&view.breadcrumb.to_uppercase(), cols.saturating_sub(9));
    p.text(x + 10.0 * u, y + (l.sub_h - gh) / 2.0, s, theme.accent, &crumb);
    let count = rows.iter().filter(|r| r.selectable()).count();
    if count > 0 {
        let pos = rows.iter().take(nav.selected() + 1).filter(|r| r.selectable()).count();
        let t = format!("{pos} / {count}");
        let tw = p.text_width(&t, s);
        p.text(x + w - 10.0 * u - tw, y + (l.sub_h - gh) / 2.0, s, theme.muted, &t);
    }
    y += l.sub_h;

    // body
    p.rect(x, y, w, filter_h + body_rows as f32 * l.row_h + 4.0 * u, theme.panel);
    if view.filterable {
        let fx = x + 8.0 * u;
        let fw = w - 16.0 * u;
        let fy = y + 2.0 * u;
        let fh = l.row_h - 3.0 * u;
        rrect(p, fx, fy, fw, fh, u, theme.track);
        let f = nav.filter();
        let caret = if (view.time * 2.0).fract() < 0.5 && !nav.editing() { "_" } else { "" };
        let (t, c) = if f.is_empty() { ("type to filter".to_string(), theme.disabled) } else { (format!("{f}{caret}"), theme.text) };
        p.text(fx + 6.0 * u, fy + (fh - gh) / 2.0, s, theme.muted, "/");
        p.text(fx + 16.0 * u, fy + (fh - gh) / 2.0, s, c, &clip_text(&t, cols.saturating_sub(4)));
        y += filter_h;
    }
    y += 2.0 * u;
    let first = nav.top().scroll.min(rows.len().saturating_sub(1));
    let vis = l.visible;
    for (k, r) in rows.iter().enumerate().skip(first).take(vis) {
        let ry = y + (k - first) as f32 * l.row_h;
        let sel = k == nav.selected() && r.selectable();
        let ty = ry + (l.row_h - gh) / 2.0;
        if sel {
            p.rect(x + 2.0 * u, ry, w - 4.0 * u, l.row_h, theme.selection);
            p.rect(x + 2.0 * u, ry, 2.0 * u, l.row_h, theme.accent);
        }
        let disabled = r.spec.reason.is_some();
        let label_c = if disabled {
            theme.disabled
        } else if r.spec.kind == Kind::Text {
            theme.muted
        } else if sel {
            argb(255, 255, 255, 255)
        } else {
            theme.text
        };
        if r.spec.kind == Kind::Separator {
            let t = r.spec.label.to_uppercase();
            let tw = p.text(x + 10.0 * u, ty, s, theme.accent_dim, &t);
            p.rect(x + 14.0 * u + tw, ry + l.row_h / 2.0, w - 24.0 * u - tw, u.max(1.0), theme.track);
            continue;
        }
        // right side widget
        let rx = x + w - 10.0 * u;
        let mut right_w = 0.0;
        let editing = sel && nav.editing();
        if editing {
            let e = nav.edit.as_ref().unwrap();
            let caret = if (view.time * 2.0).fract() < 0.5 { "_" } else { " " };
            let t = format!("{}{caret}", e.buf);
            let tw = p.text_width(&t, s).max(40.0 * u);
            rrect(p, rx - tw - 8.0 * u, ry + 2.0 * u, tw + 8.0 * u, l.row_h - 4.0 * u, u, theme.track);
            p.frame(rx - tw - 8.0 * u, ry + 2.0 * u, tw + 8.0 * u, l.row_h - 4.0 * u, u.max(1.0), theme.accent);
            p.text(rx - tw - 4.0 * u, ty, s, theme.text, &t);
            right_w = tw + 12.0 * u;
        } else if disabled {
            let t = "unavailable";
            let tw = p.text_width(t, s);
            p.text(rx - tw, ty, s, theme.disabled, t);
            right_w = tw + 6.0 * u;
        } else if let Some(rt) = &r.spec.right {
            let tw = p.text_width(rt, s);
            p.text(rx - tw, ty, s, if sel { theme.text } else { theme.accent }, rt);
            right_w = tw + 6.0 * u;
        } else {
            match r.spec.kind {
                Kind::Toggle => {
                    let on = r.spec.value != 0.0;
                    let (tw, th) = (18.0 * u, 9.0 * u);
                    let tx = rx - tw;
                    let tyy = ry + (l.row_h - th) / 2.0;
                    rrect(p, tx, tyy, tw, th, 2.0 * u, if on { theme.on } else { theme.track });
                    let kx = if on { tx + tw - 8.0 * u } else { tx + u };
                    rrect(p, kx, tyy + u, 7.0 * u, th - 2.0 * u, u, if on { theme.knob } else { theme.muted });
                    right_w = tw + 6.0 * u;
                }
                Kind::Slider => {
                    let vt = trim_num(r.spec.value);
                    let vw = p.text_width(&vt, s).max(p.text_width("0000", s));
                    p.text(rx - p.text_width(&vt, s), ty, s, if sel { theme.text } else { theme.accent }, &vt);
                    let trw = 56.0 * u;
                    let trx = rx - vw - 6.0 * u - trw;
                    let try_ = ry + l.row_h / 2.0 - u;
                    let span = r.spec.max - r.spec.min;
                    let frac = if span > 0.0 { ((r.spec.value - r.spec.min) / span).clamp(0.0, 1.0) as f32 } else { 0.0 };
                    p.rect(trx, try_, trw, 2.0 * u, theme.track);
                    p.rect(trx, try_, trw * frac, 2.0 * u, theme.accent);
                    p.rect(trx + trw * frac - u, try_ - 3.0 * u, 3.0 * u, 8.0 * u, theme.knob);
                    right_w = vw + trw + 12.0 * u;
                }
                Kind::Number | Kind::Choice => {
                    let v = if r.spec.kind == Kind::Number { trim_num(r.spec.value) } else { r.spec.options.get(r.spec.value.max(0.0) as usize).cloned().unwrap_or_default() };
                    let v = clip_text(&v, 22);
                    let arrows = if sel { theme.accent } else { theme.disabled };
                    let lw = p.text_width("<", s);
                    let vw = p.text_width(&v, s);
                    p.text(rx - lw, ty, s, arrows, ">");
                    p.text(rx - lw - 4.0 * u - vw, ty, s, if sel { theme.text } else { theme.accent }, &v);
                    p.text(rx - 2.0 * lw - 8.0 * u - vw, ty, s, arrows, "<");
                    right_w = vw + 2.0 * lw + 14.0 * u;
                }
                Kind::Input => {
                    let v = if r.spec.text.is_empty() { "type...".to_string() } else { clip_text(&r.spec.text, 20) };
                    let vw = p.text_width(&v, s).max(40.0 * u);
                    p.frame(rx - vw - 8.0 * u, ry + 2.0 * u, vw + 8.0 * u, l.row_h - 4.0 * u, u.max(1.0), if sel { theme.accent_dim } else { theme.track });
                    p.text(rx - vw - 4.0 * u, ty, s, if r.spec.text.is_empty() { theme.disabled } else { theme.text }, &v);
                    right_w = vw + 14.0 * u;
                }
                Kind::Subpage => {
                    p.text(rx - p.text_width(">", s), ty, s, if sel { theme.accent } else { theme.muted }, ">");
                    right_w = 10.0 * u;
                }
                Kind::Button => {
                    if sel {
                        let t = "run";
                        p.text(rx - p.text_width(t, s), ty, s, theme.accent, t);
                    }
                    right_w = 24.0 * u;
                }
                _ => {}
            }
        }
        let room = ((w - 14.0 * u - right_w - 10.0 * u) / adv).max(4.0) as usize;
        let mut label = r.spec.label.clone();
        if r.spec.hotkey != 0 {
            label = format!("{label}  [{}]", scripthook_sdk::keys::vk_name(r.spec.hotkey));
        }
        p.text(x + 12.0 * u, ty, s, label_c, &clip_text(&label, room));
    }
    // scroll bar
    if rows.len() > vis {
        let top = y;
        let h = vis as f32 * l.row_h;
        p.rect(x + w - 4.0 * u, top, 2.0 * u, h, theme.track);
        let th = (h * vis as f32 / rows.len() as f32).max(6.0 * u);
        let ty = top + (h - th) * (first as f32 / (rows.len() - vis) as f32);
        p.rect(x + w - 4.0 * u, ty, 2.0 * u, th, theme.accent_dim);
    }
    y += body_rows as f32 * l.row_h + 2.0 * u;

    // footer: key hints
    p.rect(x, y, w, l.footer_h, theme.footer);
    let mut fx = x + 8.0 * u;
    let fy = y + (l.footer_h - gh) / 2.0;
    for (key, what) in view.hints {
        let kw = p.text_width(key, s) + 4.0 * u;
        if fx + kw + p.text_width(what, s) + 6.0 * u > x + w - 6.0 * u {
            break;
        }
        rrect(p, fx, fy - 2.0 * u, kw, gh + 4.0 * u, u, theme.track);
        p.text(fx + 2.0 * u, fy, s, theme.text, key);
        fx += kw + 3.0 * u;
        let ww = p.text(fx, fy, s, theme.muted, what);
        fx += ww + 6.0 * u;
    }
    y += l.footer_h;

    // description panel: the selected row's reason or hint, or the page subtitle
    let sel = rows.get(nav.selected()).filter(|r| r.selectable());
    let (text, colour, label) = match sel {
        Some(r) if r.spec.reason.is_some() => (r.spec.reason.clone().unwrap_or_default(), theme.warn, "Unavailable"),
        Some(r) if !r.spec.hint.is_empty() => (r.spec.hint.clone(), theme.text, "Info"),
        _ if !view.subtitle.is_empty() => (view.subtitle.clone(), theme.muted, "Page"),
        _ => (String::new(), theme.muted, ""),
    };
    if !text.is_empty() {
        let lines = wrap(&text, cols.saturating_sub(2), 3);
        let dy = y + 4.0 * u;
        let dh = (lines.len() as f32 + 1.0) * (gh + 4.0 * u) + 6.0 * u;
        rrect(p, x, dy, w, dh, 2.0 * u, theme.panel);
        p.rect(x, dy, 2.0 * u, dh, colour);
        p.text(x + 10.0 * u, dy + 4.0 * u, s, colour, &label.to_uppercase());
        for (k, line) in lines.iter().enumerate() {
            p.text(x + 10.0 * u, dy + 4.0 * u + (k as f32 + 1.0) * (gh + 4.0 * u), s, theme.text, line);
        }
    }
    vis
}

/// Notices stacked at the bottom left (fade out in their last half second).
pub fn draw_toasts(p: &mut dyn Painter, theme: &Theme, scale: f32, toasts: &[(String, f64)]) {
    let u = scale.max(1.0);
    let (_, sh) = p.size();
    let gh = p.glyph_height(u);
    let mut y = sh as f32 - 16.0 * u;
    for (text, left) in toasts.iter().rev() {
        let a = (left.clamp(0.0, 0.5) * 2.0) as f32;
        let fade = |c: u32| -> u32 { (((c >> 24) as f32 * a) as u32) << 24 | (c & 0x00FF_FFFF) };
        let t = clip_text(text, 70);
        let w = p.text_width(&t, u) + 20.0 * u;
        let h = gh + 10.0 * u;
        y -= h;
        rrect(p, 12.0 * u, y, w, h, 2.0 * u, fade(theme.header));
        p.rect(12.0 * u, y, 2.0 * u, h, fade(theme.accent));
        p.text(22.0 * u, y + 5.0 * u, u, fade(theme.text), &t);
        y -= 4.0 * u;
    }
}

/// HUD lines at the top right.
pub fn draw_hud(p: &mut dyn Painter, theme: &Theme, scale: f32, lines: &[String]) {
    let u = scale.max(1.0);
    let (sw, _) = p.size();
    let gh = p.glyph_height(u);
    let mut y = 12.0 * u;
    for l in lines {
        let t = clip_text(l, 60);
        let w = p.text_width(&t, u) + 12.0 * u;
        let h = gh + 6.0 * u;
        rrect(p, sw as f32 - 12.0 * u - w, y, w, h, u, argb(170, 12, 16, 22));
        p.text(sw as f32 - 6.0 * u - w, y + 3.0 * u, u, theme.text, &t);
        y += h + 3.0 * u;
    }
}
