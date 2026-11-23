//! Drawing: the [`Painter`] trait over the host's canvas ([`HostCanvas`]) and a software canvas
//! ([`SoftCanvas`]) with the same font (Inter, [`crate::font`]) and metrics, for previews and
//! tests (PNG output). Text "scale" s means a pixel size of `font::px_for_scale(s)` (s = 2: 17 px).
//!
//! Colours are `0xAARRGGBB` ([`argb`]).

use crate::ffi::ShkCanvas;
use std::ffi::CString;

pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

/// Something to draw on: filled rectangles, bitmap-font text, a clip rectangle.
pub trait Painter {
    fn size(&self) -> (u32, u32);
    /// suggested integer text scale (1 below 600 lines, 2 up to 1300, 3 above)
    fn ui_scale(&self) -> f32;
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32);
    /// draws ASCII text with its top-left at (x, y); returns the width
    fn text(&mut self, x: f32, y: f32, scale: f32, c: u32, s: &str) -> f32;
    /// w <= 0 or h <= 0 removes the clip
    fn clip(&mut self, x: f32, y: f32, w: f32, h: f32);
    /// width of `s` in pixels
    fn text_width(&self, s: &str, scale: f32) -> f32;
    /// cap height in pixels (text is drawn with its cap line at y)
    fn glyph_height(&self, scale: f32) -> f32;
    /// A rectangle outline of thickness `t`.
    fn frame(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, c: u32) {
        self.rect(x, y, w, t, c);
        self.rect(x, y + h - t, w, t, c);
        self.rect(x, y + t, t, h - 2.0 * t, c);
        self.rect(x + w - t, y + t, t, h - 2.0 * t, c);
    }
}

pub fn scale_for_height(h: u32) -> f32 {
    if h < 600 {
        1.0
    } else if h <= 1300 {
        2.0
    } else {
        3.0
    }
}

/// The host's canvas during a draw callback.
pub struct HostCanvas<'a> {
    c: &'a ShkCanvas,
}

impl<'a> HostCanvas<'a> {
    pub fn new(c: &'a ShkCanvas) -> HostCanvas<'a> {
        HostCanvas { c }
    }
}

impl Painter for HostCanvas<'_> {
    fn size(&self) -> (u32, u32) {
        (self.c.width, self.c.height)
    }
    fn ui_scale(&self) -> f32 {
        self.c.ui_scale.max(1.0)
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32) {
        // SAFETY: the host's canvas during its callback.
        unsafe { (self.c.rect)(self.c.ctx, x, y, w, h, c) }
    }
    fn text(&mut self, x: f32, y: f32, scale: f32, c: u32, s: &str) -> f32 {
        let t = CString::new(s.replace('\0', "")).unwrap_or_default();
        // SAFETY: as above.
        unsafe { (self.c.text)(self.c.ctx, x, y, scale, c, t.as_ptr()) }
    }
    fn clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        // SAFETY: as above.
        unsafe { (self.c.clip)(self.c.ctx, x, y, w, h) }
    }
    fn text_width(&self, s: &str, scale: f32) -> f32 {
        let t = CString::new(s.replace('\0', "")).unwrap_or_default();
        // SAFETY: as above.
        unsafe { (self.c.measure)(self.c.ctx, scale, t.as_ptr()) }
    }
    fn glyph_height(&self, scale: f32) -> f32 {
        self.c.cap_height * scale
    }
}

// ---------------------------------------------------------------------------------------------
// Software canvas

/// An RGBA8 image you can draw on with the overlay's font (blending of `argb`).
pub struct SoftCanvas {
    pub width: u32,
    pub height: u32,
    /// RGBA, row-major
    pub pixels: Vec<u8>,
    clip: Option<(i32, i32, i32, i32)>,
    scale: f32,
}

impl SoftCanvas {
    pub fn new(width: u32, height: u32, background: u32) -> SoftCanvas {
        let mut c = SoftCanvas { width, height, pixels: vec![0; (width * height * 4) as usize], clip: None, scale: scale_for_height(height) };
        let (r, g, b) = ((background >> 16) as u8, (background >> 8) as u8, background as u8);
        for p in c.pixels.chunks_mut(4) {
            p.copy_from_slice(&[r, g, b, 255]);
        }
        c
    }

    pub fn set_ui_scale(&mut self, s: f32) {
        self.scale = s.max(1.0);
    }

    /// A vertical gradient (backgrounds of previews).
    pub fn gradient(&mut self, top: u32, bottom: u32) {
        for y in 0..self.height {
            let t = y as f32 / (self.height.max(2) - 1) as f32;
            let mix = |s: u32| -> u8 {
                let a = ((top >> s) & 0xFF) as f32;
                let b = ((bottom >> s) & 0xFF) as f32;
                (a + (b - a) * t).round() as u8
            };
            let px = [mix(16), mix(8), mix(0), 255];
            for x in 0..self.width {
                let i = ((y * self.width + x) * 4) as usize;
                self.pixels[i..i + 4].copy_from_slice(&px);
            }
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3]]
    }

    fn bounds(&self) -> (i32, i32, i32, i32) {
        self.clip.unwrap_or((0, 0, self.width as i32, self.height as i32))
    }

    fn blend(&mut self, x: i32, y: i32, c: u32) {
        let (cx0, cy0, cx1, cy1) = self.bounds();
        if x < cx0 || y < cy0 || x >= cx1 || y >= cy1 || x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let a = (c >> 24) as u32;
        if a == 0 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        let src = [(c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF];
        for k in 0..3 {
            let d = self.pixels[i + k] as u32;
            self.pixels[i + k] = ((src[k] * a + d * (255 - a) + 127) / 255) as u8;
        }
        self.pixels[i + 3] = 255;
    }

    /// The image as a PNG file.
    pub fn png(&self) -> Vec<u8> {
        crate::png::encode_rgba(self.width, self.height, &self.pixels)
    }
}

impl Painter for SoftCanvas {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    fn ui_scale(&self) -> f32 {
        self.scale
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32) {
        let (x0, y0) = (x.round() as i32, y.round() as i32);
        let (x1, y1) = ((x + w).round() as i32, (y + h).round() as i32);
        for py in y0.max(0)..y1.min(self.height as i32) {
            for px in x0.max(0)..x1.min(self.width as i32) {
                self.blend(px, py, c);
            }
        }
    }
    fn text(&mut self, x: f32, y: f32, scale: f32, c: u32, s: &str) -> f32 {
        let at = crate::font::atlas();
        let face = at.face(crate::font::px_for_scale(scale));
        let a = (c >> 24) as f32 / 255.0;
        let mut pen = x;
        for ch in s.chars() {
            let g = at.glyph(face, ch);
            let gx = (pen + g.left).round() as i32;
            let gy = (y + g.top).round() as i32;
            for yy in 0..g.h {
                for xx in 0..g.w {
                    let cov = at.pixels[((g.y + yy) * at.width + g.x + xx) as usize];
                    if cov != 0 {
                        let alpha = (cov as f32 * a).round() as u32;
                        self.blend(gx + xx as i32, gy + yy as i32, alpha << 24 | (c & 0x00FF_FFFF));
                    }
                }
            }
            pen += g.advance;
        }
        pen - x
    }
    fn clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.clip = if w <= 0.0 || h <= 0.0 { None } else { Some((x.round() as i32, y.round() as i32, (x + w).round() as i32, (y + h).round() as i32)) };
    }
    fn text_width(&self, s: &str, scale: f32) -> f32 {
        crate::font::atlas().text_width(s, scale)
    }
    fn glyph_height(&self, scale: f32) -> f32 {
        crate::font::atlas().face(crate::font::px_for_scale(scale)).cap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_canvas_draws_and_clips() {
        let mut c = SoftCanvas::new(64, 32, argb(255, 0, 0, 0));
        c.rect(2.0, 2.0, 10.0, 10.0, argb(255, 255, 0, 0));
        assert_eq!(c.pixel(5, 5), [255, 0, 0, 255]);
        c.rect(0.0, 0.0, 64.0, 32.0, argb(128, 0, 0, 255));
        let p = c.pixel(5, 5);
        assert!(p[0] > 100 && p[0] < 140 && p[2] > 100, "{p:?}");
        c.clip(20.0, 0.0, 10.0, 32.0);
        c.rect(0.0, 0.0, 64.0, 32.0, argb(255, 0, 255, 0));
        assert_eq!(c.pixel(25, 5)[1], 255);
        assert_ne!(c.pixel(5, 5)[1], 255);
        c.clip(0.0, 0.0, 0.0, 0.0);
        let w = c.text(2.0, 16.0, 2.0, argb(255, 255, 255, 255), "Hi");
        assert!(w > 10.0 && w < 30.0, "{w}");
        assert_eq!(w, c.text_width("Hi", 2.0));
        // something of the H's stem is lit
        assert!((16..30).any(|y| c.pixel(4, y)[0] > 200 || c.pixel(5, y)[0] > 200));
        let png = c.png();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }
}
