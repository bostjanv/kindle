//! Greyscale drawing surface with a small PIL-like API: shapes via tiny-skia,
//! text via ab_glyph with the bundled DejaVu fonts.
//!
//! Coordinates follow PIL: rectangles and ellipse bounding boxes are inclusive,
//! and text is placed by its top-left corner at the font's ascender line.

use std::io::Cursor;
use std::sync::OnceLock;

use ab_glyph::{Font as _, FontRef, GlyphId, PxScale, ScaleFont, point};
use anyhow::Result;
use image::{GrayImage, ImageEncoder, codecs::png};
use tiny_skia::{
    Color, FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, PremultipliedColorU8, Rect, Stroke, Transform,
};

pub const BLACK: u8 = 0;
pub const DARK: u8 = 64;
pub const MID: u8 = 128;
pub const LIGHT: u8 = 200;
pub const WHITE: u8 = 255;

static REGULAR: &[u8] = include_bytes!("../fonts/DejaVuSans.ttf");
static BOLD: &[u8] = include_bytes!("../fonts/DejaVuSans-Bold.ttf");

fn faces() -> &'static [FontRef<'static>; 2] {
    static FACES: OnceLock<[FontRef<'static>; 2]> = OnceLock::new();
    FACES.get_or_init(|| {
        [
            FontRef::try_from_slice(REGULAR).expect("bundled DejaVuSans.ttf"),
            FontRef::try_from_slice(BOLD).expect("bundled DejaVuSans-Bold.ttf"),
        ]
    })
}

/// A font face at a pixel size (the em size, as in PIL's `truetype(..., size)`).
#[derive(Clone, Copy)]
pub struct Font {
    face: &'static FontRef<'static>,
    pub size: i32,
    scale: PxScale,
    ascent: f32,
}

pub fn font(size: i32, bold: bool) -> Font {
    let face = &faces()[bold as usize];
    let size = size.max(1);
    let upem = face.units_per_em().unwrap_or(2048.0);
    Font {
        face,
        size,
        scale: PxScale::from(size as f32 * face.height_unscaled() / upem),
        ascent: (face.ascent_unscaled() * size as f32 / upem).ceil(),
    }
}

impl Font {
    /// Glyph ids and their x offsets from the text origin, plus the total advance.
    fn layout(&self, text: &str) -> (Vec<(GlyphId, f32)>, f32) {
        let sf = self.face.as_scaled(self.scale);
        let mut out = Vec::with_capacity(text.len());
        let mut x = 0.0;
        let mut prev: Option<GlyphId> = None;
        for c in text.chars() {
            let id = sf.glyph_id(c);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            out.push((id, x));
            x += sf.h_advance(id);
            prev = Some(id);
        }
        (out, x)
    }

    /// Advance width of `text` (PIL's `textlength`).
    pub fn length(&self, text: &str) -> f32 {
        self.layout(text).1
    }

    /// `(left, top, right, bottom)` of `text` drawn at (0, 0) (PIL's `textbbox`):
    /// horizontally from the origin to the advance, vertically the ink.
    pub fn bbox(&self, text: &str) -> (i32, i32, i32, i32) {
        let (glyphs, adv) = self.layout(text);
        let (mut l, mut r) = (0.0f32, adv);
        let (mut t, mut b) = (f32::MAX, f32::MIN);
        for (id, x) in glyphs {
            let g = id.with_scale_and_position(self.scale, point(x, self.ascent));
            if let Some(o) = self.face.outline_glyph(g) {
                let pb = o.px_bounds();
                l = l.min(pb.min.x);
                r = r.max(pb.max.x);
                t = t.min(pb.min.y);
                b = b.max(pb.max.y);
            }
        }
        if t > b {
            (t, b) = (0.0, 0.0);
        }
        (l.floor() as i32, t.floor() as i32, r.ceil() as i32, b.ceil() as i32)
    }

    /// Shortens `text` with "…" until it fits in `max_w` pixels.
    pub fn ellipsize(&self, text: &str, max_w: i32) -> String {
        let max_w = max_w as f32;
        if self.length(text) <= max_w {
            return text.to_string();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while !chars.is_empty() && self.length(&format!("{}…", chars.iter().collect::<String>())) > max_w {
            chars.pop();
        }
        format!("{}…", chars.iter().collect::<String>().trim_end())
    }
}

/// A widget's area: `x0..x1` × `y0..y1` (exclusive ends).
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Bounds {
    pub fn w(&self) -> i32 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> i32 {
        self.y1 - self.y0
    }
}

pub struct Canvas {
    pix: Pixmap,
}

fn paint(v: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(v, v, v, 255));
    p.anti_alias = true;
    p
}

/// PIL addresses pixels, tiny-skia a continuous plane: an odd-width line through
/// pixel row y must run along y + 0.5 to stay crisp.
fn line_offset(width: f32) -> f32 {
    if (width.round() as i32) % 2 == 1 { 0.5 } else { 0.0 }
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Canvas {
        let mut pix = Pixmap::new(w.max(1), h.max(1)).expect("canvas size");
        pix.fill(Color::WHITE);
        Canvas { pix }
    }

    fn fill(&mut self, path: &Path, v: u8) {
        self.pix
            .fill_path(path, &paint(v), FillRule::Winding, Transform::identity(), None);
    }

    fn stroke(&mut self, path: &Path, v: u8, width: f32) {
        let stroke = Stroke {
            width,
            line_cap: LineCap::Butt,
            ..Stroke::default()
        };
        self.pix
            .stroke_path(path, &paint(v), &stroke, Transform::identity(), None);
    }

    /// Filled rectangle, inclusive corners.
    pub fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, v: u8) {
        if let Some(r) = Rect::from_ltrb(x0, y0, x1 + 1.0, y1 + 1.0) {
            self.pix.fill_rect(r, &paint(v), Transform::identity(), None);
        }
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, v: u8, width: f32) {
        let o = line_offset(width);
        let mut pb = PathBuilder::new();
        pb.move_to(x0 + o, y0 + o);
        pb.line_to(x1 + o, y1 + o);
        if let Some(p) = pb.finish() {
            self.stroke(&p, v, width);
        }
    }

    pub fn polygon(&mut self, pts: &[(f32, f32)], v: u8) {
        let mut pb = PathBuilder::new();
        for (i, &(x, y)) in pts.iter().enumerate() {
            if i == 0 {
                pb.move_to(x + 0.5, y + 0.5)
            } else {
                pb.line_to(x + 0.5, y + 0.5)
            }
        }
        pb.close();
        if let Some(p) = pb.finish() {
            self.fill(&p, v);
        }
    }

    /// Ellipse in the inclusive bounding box; `outline` is (value, width), drawn inside the box.
    pub fn ellipse(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, fill: Option<u8>, outline: Option<(u8, f32)>) {
        let (x1, y1) = (x1 + 1.0, y1 + 1.0);
        if let Some(v) = fill
            && let Some(p) = Rect::from_ltrb(x0, y0, x1, y1).and_then(PathBuilder::from_oval)
        {
            self.fill(&p, v);
        }
        if let Some((v, w)) = outline {
            let h = w / 2.0;
            if let Some(p) = Rect::from_ltrb(x0 + h, y0 + h, x1 - h, y1 - h).and_then(PathBuilder::from_oval) {
                self.stroke(&p, v, w);
            }
        }
    }

    /// Outline of a rounded rectangle, inclusive corners, stroke drawn inside the box.
    #[allow(clippy::too_many_arguments)]
    pub fn rounded_rect_outline(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, v: u8, width: f32) {
        let h = width / 2.0;
        let (l, t, r, b) = (x0 + h, y0 + h, x1 + 1.0 - h, y1 + 1.0 - h);
        let rad = (radius - h).max(0.0).min((r - l) / 2.0).min((b - t) / 2.0);
        let mut pb = PathBuilder::new();
        pb.move_to(l + rad, t);
        pb.line_to(r - rad, t);
        pb.quad_to(r, t, r, t + rad);
        pb.line_to(r, b - rad);
        pb.quad_to(r, b, r - rad, b);
        pb.line_to(l + rad, b);
        pb.quad_to(l, b, l, b - rad);
        pb.line_to(l, t + rad);
        pb.quad_to(l, t, l + rad, t);
        pb.close();
        if let Some(p) = pb.finish() {
            self.stroke(&p, v, width);
        }
    }

    fn blend(&mut self, x: i32, y: i32, v: u8, coverage: f32) {
        let (w, h) = (self.pix.width() as i32, self.pix.height() as i32);
        if x < 0 || y < 0 || x >= w || y >= h || coverage <= 0.0 {
            return;
        }
        let px = &mut self.pix.pixels_mut()[(y * w + x) as usize];
        let c = coverage.min(1.0);
        let g = (px.red() as f32 * (1.0 - c) + v as f32 * c).round() as u8;
        *px = PremultipliedColorU8::from_rgba(g, g, g, 255).expect("opaque");
    }

    /// Draws `text` with its top-left at (x, y), the top being the font's ascender line.
    pub fn text(&mut self, x: f32, y: f32, text: &str, f: &Font, v: u8) {
        let (glyphs, _) = f.layout(text);
        for (id, gx) in glyphs {
            let g = id.with_scale_and_position(f.scale, point(x + gx, y + f.ascent));
            if let Some(o) = f.face.outline_glyph(g) {
                let pb = o.px_bounds();
                let (ox, oy) = (pb.min.x as i32, pb.min.y as i32);
                o.draw(|px, py, c| self.blend(ox + px as i32, oy + py as i32, v, c));
            }
        }
    }

    /// Copies a greyscale image onto the canvas with its top-left at (x, y).
    pub fn paste(&mut self, img: &GrayImage, x: i32, y: i32) {
        let w = self.pix.width() as i32;
        let h = self.pix.height() as i32;
        let pixels = self.pix.pixels_mut();
        for (px, py, p) in img.enumerate_pixels() {
            let (cx, cy) = (x + px as i32, y + py as i32);
            if cx >= 0 && cy >= 0 && cx < w && cy < h {
                let g = p.0[0];
                pixels[(cy * w + cx) as usize] = PremultipliedColorU8::from_rgba(g, g, g, 255).expect("opaque");
            }
        }
    }

    pub fn into_gray(self) -> GrayImage {
        let (w, h) = (self.pix.width(), self.pix.height());
        GrayImage::from_raw(w, h, self.pix.pixels().iter().map(|p| p.red()).collect()).expect("size")
    }
}

/// 8-bit greyscale, no alpha, no interlace: the format `eips -g` on the Kindle Keyboard accepts.
pub fn encode_png(img: &GrayImage) -> Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    png::PngEncoder::new_with_quality(&mut buf, png::CompressionType::Best, png::FilterType::Adaptive).write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::L8,
    )?;
    Ok(buf.into_inner())
}
