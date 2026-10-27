//! Panels' background images, decoded with WIC at the size they're shown:
//! a panel's picture (cropped to what shows, never bigger than the file) and,
//! for blur and pixelate, each monitor's wallpaper as Windows shows it, which
//! every panel on that monitor draws its part of. Blurred and pixelated
//! images are decoded smaller, since the effect hides the detail anyway.
//!
//! Windows' own blur (DWM's blur-behind) would be live, but it turns into
//! solid black when "Transparency effects" is off, and it can't pixelate.

use std::path::Path;

use sorter::background::{WallFit, blur, decode_plan, wallpaper_plan};
use sorter::model::ImageFit;
use sorter::panel_layout::Rect;
use windows::Win32::Foundation::{GENERIC_READ, RECT};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapFrameDecode,
    IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand, WICRect,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

use super::Wide;
use super::render::Gfx;
use super::wallpaper::Wall;

/// The pixels of every cached image together.
const MAX_BYTES: usize = 8 << 20;

/// Largest wallpaper tile decoded whole (tiles are usually small).
const MAX_TILE_PIXELS: u32 = 4096 * 4096;

/// What an image is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A panel's picture, placed by `fit`.
    Picture(ImageFit),
    /// The wallpaper of the monitor at `rect` (screen pixels), placed by
    /// Windows in `area` with `fit` on a `color` background (0x00bbggrr).
    Wallpaper { rect: (i32, i32, i32, i32), area: (i32, i32, i32, i32), fit: WallFit, color: u32 },
}

/// What an image was decoded for. Anything different needs a new decode.
#[derive(Clone, Debug, PartialEq)]
pub struct Want {
    /// Lower-cased path; empty for a plain-color wallpaper.
    pub path: String,
    pub mtime: f64,
    pub kind: Kind,
    /// The size it shows at in pixels: the panel's, or the monitor's.
    pub target: (u32, u32),
    /// Display scale in percent, for tiling.
    pub scale: u32,
    /// Blur radius in pixels; 0 for none.
    pub blur: u32,
    /// Decoded this many times smaller than it shows.
    pub reduce: u32,
}

fn mtime(path: &str) -> f64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64())
}

fn xywh(r: RECT) -> (i32, i32, i32, i32) {
    (r.left, r.top, r.right - r.left, r.bottom - r.top)
}

impl Want {
    pub fn picture(path: &str, fit: ImageFit, target: (u32, u32), scale: f32) -> Self {
        Self {
            path: path.to_lowercase(),
            mtime: mtime(path),
            kind: Kind::Picture(fit),
            target,
            scale: (scale * 100.0).round() as u32,
            blur: 0,
            reduce: 1,
        }
    }

    /// `wall` over its whole monitor, as Windows shows it.
    pub fn wallpaper(wall: &Wall, scale: f32) -> Self {
        let p = xywh(wall.monitor);
        Self {
            path: wall.path.to_lowercase(),
            mtime: if wall.path.is_empty() { 0.0 } else { mtime(&wall.path) },
            kind: Kind::Wallpaper {
                rect: p,
                area: xywh(wall.area),
                fit: wall.fit,
                color: wall.color,
            },
            target: (p.2.max(1) as u32, p.3.max(1) as u32),
            scale: (scale * 100.0).round() as u32,
            blur: 0,
            reduce: 1,
        }
    }

    /// Blurred by `radius` pixels, decoded `reduce` times smaller.
    pub fn blurred(self, radius: u32, reduce: u32) -> Self {
        Self { blur: radius, reduce: reduce.max(1), ..self }
    }

    pub fn reduced(self, reduce: u32) -> Self {
        Self { reduce: reduce.max(1), ..self }
    }
}

/// A decoded image and how many DIPs one of its pixels covers when tiled.
#[derive(Clone)]
pub struct Image {
    pub bitmap: ID2D1Bitmap,
    pub dip_per_px: f32,
}

/// The cache slot of a monitor's wallpaper as `want` has it (a panel's
/// picture's slot is the panel's id). Panels on one monitor with the same
/// effect and strength share it.
pub fn wall_slot(want: &Want) -> String {
    let rect = match want.kind {
        Kind::Wallpaper { rect, .. } => rect,
        Kind::Picture(_) => (0, 0, 0, 0),
    };
    format!("\0wall {} {} {} {}", rect.0, rect.1, want.blur, want.reduce)
}

struct Entry {
    slot: String,
    want: Want,
    /// None when the file couldn't be read.
    image: Option<Image>,
    bytes: usize,
    used: u64,
}

/// Background images by slot, one each. A slot's new image (or new size)
/// replaces its old one; past `MAX_BYTES` the least recently drawn go first.
#[derive(Default)]
pub struct BgImages {
    entries: Vec<Entry>,
    bytes: usize,
    tick: u64,
    generation: u32,
}

impl BgImages {
    fn check(&mut self, gfx: &Gfx) {
        if self.generation != gfx.generation {
            *self = Self { generation: gfx.generation, ..Self::default() };
        }
    }

    /// The image in `slot`, and whether it was decoded for `want`. One made
    /// for something else (another size, say) still draws until it's replaced.
    pub fn get(&mut self, gfx: &Gfx, slot: &str, want: &Want) -> (Option<Image>, bool) {
        self.check(gfx);
        self.tick += 1;
        let tick = self.tick;
        match self.entries.iter_mut().find(|e| e.slot == slot) {
            Some(e) => {
                e.used = tick;
                (e.image.clone(), e.want == *want)
            }
            None => (None, false),
        }
    }

    /// Whatever image `slot` has, for the settings preview.
    pub fn peek(&self, gfx: &Gfx, slot: &str) -> Option<&Image> {
        if self.generation != gfx.generation {
            return None;
        }
        self.entries.iter().find(|e| e.slot == slot)?.image.as_ref()
    }

    /// Decodes `want` (from the file at `original_path`) into `slot`, replacing what it had.
    pub fn load(&mut self, gfx: &Gfx, slot: &str, original_path: &str, want: Want) {
        self.check(gfx);
        self.drop_slot(slot);
        let decoded = decode(Path::new(original_path), &want);
        let bytes = decoded.as_ref().map_or(0, |d| d.data.len());
        let image = decoded.and_then(|d| {
            let bitmap = gfx.bitmap(d.w, d.h, &d.data).ok()?;
            Some(Image { bitmap, dip_per_px: d.dip_per_px })
        });
        self.tick += 1;
        self.entries.push(Entry { slot: slot.to_string(), want, image, bytes, used: self.tick });
        self.bytes += bytes;
        while self.bytes > MAX_BYTES && self.entries.len() > 1 {
            let (i, _) =
                self.entries.iter().enumerate().min_by_key(|(_, e)| e.used).expect("not empty");
            let old = self.entries.swap_remove(i);
            self.bytes -= old.bytes;
        }
    }

    pub fn drop_slot(&mut self, slot: &str) {
        if let Some(i) = self.entries.iter().position(|e| e.slot == slot) {
            let old = self.entries.swap_remove(i);
            self.bytes -= old.bytes;
        }
    }

    /// Drops panel `panel`'s picture (it was deleted, or got another one).
    pub fn forget(&mut self, panel: &str) {
        self.drop_slot(panel);
    }

    /// Drops the wallpaper images no panel uses any more (another strength, say).
    pub fn retain_walls(&mut self, used: &[String]) {
        let mut freed = 0;
        self.entries.retain(|e| {
            let keep = !matches!(e.want.kind, Kind::Wallpaper { .. }) || used.contains(&e.slot);
            if !keep {
                freed += e.bytes;
            }
            keep
        });
        self.bytes -= freed;
    }

    /// Drops every image, e.g. when the panels are hidden.
    pub fn clear(&mut self) {
        let generation = self.generation;
        *self = Self { generation, ..Self::default() };
    }
}

struct Decoded {
    w: u32,
    h: u32,
    dip_per_px: f32,
    /// Premultiplied BGRA.
    data: Vec<u8>,
}

fn factory() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() }
}

fn open(factory: &IWICImagingFactory, path: &Path) -> Option<(IWICBitmapFrameDecode, u32, u32)> {
    let w = Wide::os(path.as_os_str());
    unsafe {
        let decoder = factory
            .CreateDecoderFromFilename(
                w.pcwstr(),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut sw, mut sh) = (0, 0);
        frame.GetSize(&mut sw, &mut sh).ok()?;
        Some((frame, sw, sh))
    }
}

/// The `crop` (x, y, width, height) of `frame`, scaled to `size`, as premultiplied BGRA.
fn pixels(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
    crop: (u32, u32, u32, u32),
    size: (u32, u32),
) -> Option<Vec<u8>> {
    let (x, y, cw, ch) = crop;
    let (w, h) = (size.0.max(1), size.1.max(1));
    unsafe {
        let clipper = factory.CreateBitmapClipper().ok()?;
        let rect = WICRect { X: x as i32, Y: y as i32, Width: cw as i32, Height: ch as i32 };
        clipper.Initialize(frame, &rect).ok()?;
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler.Initialize(&clipper, w, h, WICBitmapInterpolationModeFant).ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut data = vec![0u8; (w * h * 4) as usize];
        converter.CopyPixels(std::ptr::null(), w * 4, &mut data).ok()?;
        Some(data)
    }
}

fn decode(path: &Path, want: &Want) -> Option<Decoded> {
    let r = want.reduce.max(1);
    let mut d = match &want.kind {
        Kind::Picture(fit) => {
            let factory = factory()?;
            let (frame, sw, sh) = open(&factory, path)?;
            let plan = decode_plan((sw, sh), want.target, *fit, want.scale as f32 / 100.0);
            let size = ((plan.size.0 / r).max(1), (plan.size.1 / r).max(1));
            let data = pixels(&factory, &frame, plan.crop, size)?;
            let dip_per_px = plan.dip_per_px * plan.size.0 as f32 / size.0 as f32;
            Decoded { w: size.0, h: size.1, dip_per_px, data }
        }
        Kind::Wallpaper { rect, area, fit, color } => {
            wallpaper(path, want, *rect, *area, *fit, *color)?
        }
    };
    if want.blur > 0 {
        blur(&mut d.data, d.w as usize, d.h as usize, want.blur as f32 / r as f32);
    }
    Some(d)
}

/// The wallpaper over `at` (a monitor): its color, with the picture where it shows.
fn wallpaper(
    path: &Path,
    want: &Want,
    at: (i32, i32, i32, i32),
    area: (i32, i32, i32, i32),
    fit: WallFit,
    color: u32,
) -> Option<Decoded> {
    let r = want.reduce.max(1);
    let (w, h) = (want.target.0.div_ceil(r).max(1), want.target.1.div_ceil(r).max(1));
    let bg = [(color >> 16) as u8, (color >> 8) as u8, color as u8, 255];
    let mut data = bg.repeat((w * h) as usize);
    let rect =
        |(x, y, w, h): (i32, i32, i32, i32)| Rect::new(x as f32, y as f32, w as f32, h as f32);
    if !want.path.is_empty()
        && let Some(factory) = factory()
        && let Some((frame, sw, sh)) = open(&factory, path)
    {
        if fit == WallFit::Tile {
            // Tiles repeat from the area's corner at their own size.
            if sw * sh <= MAX_TILE_PIXELS
                && let Some(tile) = pixels(&factory, &frame, (0, 0, sw, sh), (sw, sh))
            {
                for oy in 0..h {
                    let sy = (at.1 + (oy * r) as i32 - area.1).rem_euclid(sh as i32) as u32;
                    for ox in 0..w {
                        let sx = (at.0 + (ox * r) as i32 - area.0).rem_euclid(sw as i32) as u32;
                        let (s, o) = (((sy * sw + sx) * 4) as usize, ((oy * w + ox) * 4) as usize);
                        data[o..o + 4].copy_from_slice(&tile[s..s + 4]);
                    }
                }
            }
        } else if let Some(plan) = wallpaper_plan((sw, sh), rect(area), rect(at), fit) {
            let k = r as f32;
            let (dx, dy) = ((plan.dest.x / k) as u32, (plan.dest.y / k) as u32);
            let dw = ((plan.dest.w / k).round() as u32).clamp(1, w.saturating_sub(dx).max(1));
            let dh = ((plan.dest.h / k).round() as u32).clamp(1, h.saturating_sub(dy).max(1));
            if let Some(part) = pixels(&factory, &frame, plan.crop, (dw, dh)) {
                for row in 0..dh.min(h.saturating_sub(dy)) {
                    let cols = dw.min(w.saturating_sub(dx)) as usize * 4;
                    let s = (row * dw * 4) as usize;
                    let o = (((dy + row) * w + dx) * 4) as usize;
                    data[o..o + cols].copy_from_slice(&part[s..s + cols]);
                }
            }
        }
    }
    Some(Decoded { w, h, dip_per_px: 1.0, data })
}
