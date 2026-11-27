//! xkcd comic (latest or random), scaled to fit its box.

use anyhow::{Context as _, Result};
use image::{DynamicImage, GrayImage, imageops::FilterType};
use serde_json::Value;
use toml::Table;

use super::Widget;
use crate::cache::Cached;
use crate::canvas::{BLACK, Bounds, Canvas, DARK, font};
use crate::render::Context;
use crate::{config, http};

struct Comic {
    num: i64,
    title: String,
    alt: String,
    image: GrayImage,
}

pub struct Xkcd {
    random: bool,
    show_title: bool,
    show_alt: bool,
    max_upscale: f64,
    /// Random mode: prefer comics at least this wide relative to their height
    /// (0 accepts any shape, e.g. when xkcd has a big box of its own).
    min_aspect: f64,
    comic: Cached<Comic>,
}

/// Flattens transparency onto white and converts to greyscale.
fn to_gray(img: DynamicImage) -> GrayImage {
    if !img.color().has_alpha() {
        return img.to_luma8();
    }
    let la = img.to_luma_alpha8();
    GrayImage::from_fn(la.width(), la.height(), |x, y| {
        let [l, a] = la.get_pixel(x, y).0;
        let a = a as u32;
        image::Luma([((l as u32 * a + 255 * (255 - a) + 127) / 255) as u8])
    })
}

fn get_image(info: &Value) -> Result<GrayImage> {
    let url = info["img"].as_str().context("xkcd: no image url")?;
    let bytes = http::get_bytes(url, 20)?;
    Ok(to_gray(
        image::load_from_memory(&bytes).with_context(|| format!("decoding {url}"))?,
    ))
}

/// PIL's `ImageOps.autocontrast(img, cutoff=1)`: clip 1% at each end of the histogram, stretch the rest.
fn autocontrast(img: &mut GrayImage) {
    let mut h = [0u64; 256];
    for p in img.pixels() {
        h[p.0[0] as usize] += 1;
    }
    let n: u64 = h.iter().sum();
    for order in [false, true] {
        let mut cut = n / 100;
        for k in 0..256 {
            let i = if order { 255 - k } else { k };
            if cut > h[i] {
                cut -= h[i];
                h[i] = 0;
            } else {
                h[i] -= cut;
                cut = 0;
            }
            if cut == 0 {
                break;
            }
        }
    }
    let Some(lo) = (0..256).find(|&i| h[i] > 0) else { return };
    let hi = (0..256).rev().find(|&i| h[i] > 0).unwrap_or(255);
    if hi <= lo {
        return;
    }
    let scale = 255.0 / (hi - lo) as f64;
    let offset = -(lo as f64) * scale;
    let lut: Vec<u8> = (0..256)
        .map(|i| (i as f64 * scale + offset).clamp(0.0, 255.0) as u8)
        .collect();
    for p in img.pixels_mut() {
        p.0[0] = lut[p.0[0] as usize];
    }
}

/// Greedy word wrap at `width` characters (like Python's `textwrap.wrap`).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(word.drain(..width).collect());
        }
        let word: String = word.into_iter().collect();
        if word.is_empty() {
            continue;
        }
        if line.is_empty() {
            line = word;
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(&word);
        } else {
            lines.push(std::mem::replace(&mut line, word));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

impl Xkcd {
    pub fn new(cfg: &Table) -> Xkcd {
        Xkcd {
            random: config::str_or(cfg, "mode", "random") == "random",
            show_title: config::bool_or(cfg, "show_title", true),
            show_alt: config::bool_or(cfg, "show_alt", false),
            max_upscale: config::f64_or(cfg, "max_upscale", 2.0),
            min_aspect: config::f64_or(cfg, "min_aspect", 1.3),
            comic: Cached::new(config::f64_or(cfg, "refresh_minutes", 60.0) * 60.0),
        }
    }

    fn fetch(random: bool, min_aspect: f64) -> Result<Comic> {
        let latest = http::get_json("https://xkcd.com/info.0.json", &[], 15)?;
        let (mut info, mut pic) = (latest.clone(), None);
        if random {
            // Some comics are interactive/missing, and tall ones would be tiny in a
            // short box, so try a few and keep the first that fits well.
            let max = latest["num"].as_i64().context("xkcd: no comic number")?.max(1);
            for _ in 0..8 {
                let n = rand::random_range(1..=max);
                let Some(cand) = http::get_json_opt(&format!("https://xkcd.com/{n}/info.0.json"), 15)? else {
                    continue;
                };
                let img = cand["img"].as_str().unwrap_or("").to_lowercase();
                if ![".png", ".jpg", ".gif"].iter().any(|e| img.ends_with(e)) {
                    continue;
                }
                let p = get_image(&cand)?;
                let wide = p.width() as f64 / p.height() as f64 >= min_aspect;
                (info, pic) = (cand, Some(p));
                if wide {
                    break;
                }
            }
        }
        let image = match pic {
            Some(p) => p,
            None => get_image(&info)?,
        };
        let text = |k: &str| info[k].as_str().unwrap_or("").to_string();
        Ok(Comic {
            num: info["num"].as_i64().unwrap_or(0),
            title: text("safe_title"),
            alt: text("alt"),
            image,
        })
    }
}

impl Widget for Xkcd {
    fn name(&self) -> &'static str {
        "Xkcd"
    }

    fn draw(&mut self, c: &mut Canvas, b: Bounds, _ctx: &Context) -> Result<()> {
        let (random, min_aspect) = (self.random, self.min_aspect);
        let comic = self.comic.get(|| Self::fetch(random, min_aspect))?;
        let pad = 16;
        let (mut y0, mut y1) = (b.y0 + pad, b.y1 - pad);
        if self.show_title {
            let tf = font(19, true);
            let title = tf.ellipsize(&format!("xkcd #{}: {}", comic.num, comic.title), b.w() - 2 * pad);
            c.text(
                (b.x0 as f32 + (b.w() as f32 - tf.length(&title)) / 2.0).floor(),
                y0 as f32,
                &title,
                &tf,
                BLACK,
            );
            y0 += tf.size + 10;
        }
        let af = font(14, false);
        let mut alt_lines = Vec::new();
        if self.show_alt && !comic.alt.is_empty() {
            let chars = 20.max(((b.w() - 2 * pad) as f32 / (af.size as f32 * 0.55)) as usize);
            alt_lines = wrap(&comic.alt, chars);
            alt_lines.truncate(3);
            y1 -= alt_lines.len() as i32 * (af.size + 4) + 6;
        }

        let pic = &comic.image;
        let (avail_w, avail_h) = (b.w() - 2 * pad, 10.max(y1 - y0));
        let scale = (avail_w as f64 / pic.width() as f64)
            .min(avail_h as f64 / pic.height() as f64)
            .min(self.max_upscale);
        let size = (
            1.max((pic.width() as f64 * scale) as u32),
            1.max((pic.height() as f64 * scale) as u32),
        );
        let mut scaled = image::imageops::resize(pic, size.0, size.1, FilterType::Lanczos3);
        autocontrast(&mut scaled); // crisp black lines on e-ink
        c.paste(
            &scaled,
            b.x0 + (b.w() - size.0 as i32).div_euclid(2),
            y0 + (avail_h - size.1 as i32).div_euclid(2),
        );

        let mut y = y1 + 6;
        for line in &alt_lines {
            c.text(
                b.x0 as f32 + (b.w() as f32 - af.length(line)) / 2.0,
                y as f32,
                line,
                &af,
                DARK,
            );
            y += af.size + 4;
        }
        Ok(())
    }
}
