//! Screen capture, settling, numbered element tags and secret masking.

use crate::a11y::Elem;
use ab_glyph::{FontVec, PxScale};
use anyhow::{anyhow, Result};
use base64::Engine;
use image::{imageops::FilterType, Rgb, RgbImage, RgbaImage};
use imageproc::rect::Rect;
use std::sync::OnceLock;

pub struct Shot {
    pub img: RgbaImage,
    pub mon_x: i32,
    pub mon_y: i32,
}

impl Shot {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.mon_x && y >= self.mon_y && x < self.mon_x + self.img.width() as i32 && y < self.mon_y + self.img.height() as i32
    }
}

pub fn cursor() -> (i32, i32) {
    use enigo::{Enigo, Mouse, Settings};
    Enigo::new(&Settings::default()).ok().and_then(|e| e.location().ok()).unwrap_or((0, 0))
}

/// Captures the monitor the cursor is on (multi-monitor aware).
pub fn capture() -> Result<Shot> {
    let (cx, cy) = cursor();
    let mon = match xcap::Monitor::from_point(cx, cy) {
        Ok(m) => m,
        Err(_) => {
            let all = xcap::Monitor::all().map_err(|e| anyhow!("Screen capture is not available: {e}"))?;
            let idx = all.iter().position(|m| m.is_primary().unwrap_or(false)).unwrap_or(0);
            all.into_iter().nth(idx).ok_or_else(|| anyhow!("No monitor found"))?
        }
    };
    let img = mon.capture_image().map_err(|e| anyhow!("Screen capture failed (permission?): {e}"))?;
    Ok(Shot {
        mon_x: mon.x().unwrap_or(0),
        mon_y: mon.y().unwrap_or(0),
        img,
    })
}

/// 32x18 grayscale thumbnail used to detect change and settling cheaply.
pub fn fingerprint(img: &RgbaImage) -> Vec<u8> {
    image::imageops::resize(&image::DynamicImage::ImageRgba8(img.clone()).to_luma8(), 64, 36, FilterType::Triangle).into_raw()
}

pub fn diff(a: &[u8], b: &[u8]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }
    let changed = a.iter().zip(b).filter(|(x, y)| (**x as i16 - **y as i16).abs() > 12).count();
    changed as f32 / a.len() as f32
}

/// Waits until two consecutive frames match, up to `max_ms`.
pub fn settle(max_ms: u64, stop: &dyn Fn() -> bool) -> Result<Shot> {
    let start = std::time::Instant::now();
    let mut prev = capture()?;
    let mut fp = fingerprint(&prev.img);
    loop {
        if stop() || start.elapsed().as_millis() as u64 >= max_ms {
            return Ok(prev);
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        let next = capture()?;
        let nfp = fingerprint(&next.img);
        let d = diff(&fp, &nfp);
        prev = next;
        fp = nfp;
        if d < 0.004 {
            return Ok(prev);
        }
    }
}

fn font() -> Option<&'static FontVec> {
    static FONT: OnceLock<Option<FontVec>> = OnceLock::new();
    FONT.get_or_init(|| {
        let candidates = [
            "C:\\Windows\\Fonts\\segoeuib.ttf",
            "C:\\Windows\\Fonts\\arialbd.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            "/Library/Fonts/Arial.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
        ];
        candidates.iter().find_map(|p| std::fs::read(p).ok().and_then(|b| FontVec::try_from_vec(b).ok()))
    })
    .as_ref()
}

const TAG_COLORS: [[u8; 3]; 6] = [[230, 57, 70], [29, 120, 230], [20, 160, 90], [200, 110, 0], [140, 60, 200], [0, 150, 160]];

pub struct Prepared {
    pub b64: String,
    pub width: u32,
    pub height: u32,
    /// screen pixels per sent-image pixel
    pub factor: f32,
    pub jpeg: Vec<u8>,
}

/// Masks secret fields, scales the screenshot down and overlays numbered tags.
pub fn prepare(shot: &Shot, elems: &[Elem], max_w: u32, tags: bool, mask: bool, masks_extra: &[[i32; 4]]) -> Prepared {
    let mut rgb: RgbImage = image::DynamicImage::ImageRgba8(shot.img.clone()).to_rgb8();
    // Mask before scaling so secrets never reach any output.
    if mask {
        for e in elems.iter().filter(|e| e.password) {
            fill(&mut rgb, shot, e.rect, Rgb([40, 40, 40]));
        }
        for r in masks_extra {
            fill(&mut rgb, shot, *r, Rgb([40, 40, 40]));
        }
    }
    let (w, h) = rgb.dimensions();
    let factor = if w > max_w { w as f32 / max_w as f32 } else { 1.0 };
    let (nw, nh) = ((w as f32 / factor) as u32, (h as f32 / factor) as u32);
    let mut small = if factor > 1.0 { image::imageops::resize(&rgb, nw, nh, FilterType::Triangle) } else { rgb };
    if tags {
        let f = font();
        for e in elems {
            let x0 = ((e.rect[0] - shot.mon_x) as f32 / factor) as i32;
            let y0 = ((e.rect[1] - shot.mon_y) as f32 / factor) as i32;
            let x1 = ((e.rect[2] - shot.mon_x) as f32 / factor) as i32;
            let y1 = ((e.rect[3] - shot.mon_y) as f32 / factor) as i32;
            if x1 <= x0 || y1 <= y0 || x1 < 0 || y1 < 0 || x0 > nw as i32 || y0 > nh as i32 {
                continue;
            }
            let c = TAG_COLORS[e.id % TAG_COLORS.len()];
            let col = Rgb(c);
            imageproc::drawing::draw_hollow_rect_mut(&mut small, Rect::at(x0, y0).of_size((x1 - x0) as u32, (y1 - y0) as u32), col);
            let label = e.id.to_string();
            let lw = 7 * label.len() as i32 + 6;
            let ly = (y0 - 14).max(0);
            imageproc::drawing::draw_filled_rect_mut(&mut small, Rect::at(x0, ly).of_size(lw as u32, 14), col);
            if let Some(f) = f {
                imageproc::drawing::draw_text_mut(&mut small, Rgb([255, 255, 255]), x0 + 3, ly, PxScale::from(13.0), f, &label);
            }
        }
    }
    let jpeg = encode_jpeg(&small, 72);
    Prepared { b64: base64::engine::general_purpose::STANDARD.encode(&jpeg), width: nw, height: nh, factor, jpeg }
}

fn fill(img: &mut RgbImage, shot: &Shot, r: [i32; 4], c: Rgb<u8>) {
    let x0 = (r[0] - shot.mon_x).max(0);
    let y0 = (r[1] - shot.mon_y).max(0);
    let x1 = (r[2] - shot.mon_x).min(img.width() as i32);
    let y1 = (r[3] - shot.mon_y).min(img.height() as i32);
    if x1 > x0 && y1 > y0 {
        imageproc::drawing::draw_filled_rect_mut(img, Rect::at(x0, y0).of_size((x1 - x0) as u32, (y1 - y0) as u32), c);
    }
}

pub fn encode_jpeg(img: &RgbImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    let _ = enc.encode_image(img);
    out
}

/// Plain screenshot for attaching to chat.
pub fn capture_for_chat(max_w: u32) -> Result<String> {
    let s = capture()?;
    Ok(prepare(&s, &[], max_w, false, false, &[]).b64)
}
