//! What a panel's background image, texture and effect look like, as plain
//! geometry: where an image goes, the shapes of each texture's tile, which
//! tiles cover a panel, the grain effect's noise. The renderer draws from this.

use crate::model::{BgTexture, ImageFit};
use crate::panel_layout::Rect;

/// A point in a texture tile, 0–1 across the cell (a little outside is fine).
pub type Pt = (f32, f32);

/// One piece of a texture tile, in cell units. Everything is drawn in the
/// texture's color; the whole texture is faded at once, so overlaps don't darken.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A filled ellipse at `c`, turned `rot` degrees.
    Ellipse { c: Pt, rx: f32, ry: f32, rot: f32 },
    /// A line `w` thick.
    Line { a: Pt, b: Pt, w: f32 },
    /// Cubic Bézier curves from `start`, each segment two control points and
    /// an end. Filled (closed) when `w` is 0, otherwise stroked `w` thick.
    Curve { start: Pt, segs: &'static [[Pt; 3]], w: f32 },
    /// A filled polygon.
    Poly(&'static [Pt]),
}

/// A texture's repeating cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    /// Cell size in DIPs.
    pub cell: f32,
    /// Odd rows shift by half a cell, so the pattern doesn't look like a grid.
    pub stagger: bool,
    pub shapes: &'static [Shape],
}

const FLOWERS: &[Shape] = &[
    // Five petals around the middle, each pointing out.
    Shape::Ellipse { c: (0.5, 0.34), rx: 0.12, ry: 0.07, rot: -90.0 },
    Shape::Ellipse { c: (0.652, 0.451), rx: 0.12, ry: 0.07, rot: -18.0 },
    Shape::Ellipse { c: (0.594, 0.629), rx: 0.12, ry: 0.07, rot: 54.0 },
    Shape::Ellipse { c: (0.406, 0.629), rx: 0.12, ry: 0.07, rot: 126.0 },
    Shape::Ellipse { c: (0.348, 0.451), rx: 0.12, ry: 0.07, rot: 198.0 },
    Shape::Ellipse { c: (0.5, 0.5), rx: 0.035, ry: 0.035, rot: 0.0 },
    // A bud between flowers.
    Shape::Ellipse { c: (0.0, 0.0), rx: 0.03, ry: 0.03, rot: 0.0 },
];

/// The stem leaves and enters the cell at the same angle, so stacked cells make one vine.
const VINES: &[Shape] = &[
    Shape::Curve { start: (0.5, 0.0), segs: &[[(0.8, 0.25), (0.2, 0.75), (0.5, 1.0)]], w: 0.025 },
    Shape::Ellipse { c: (0.68, 0.18), rx: 0.1, ry: 0.045, rot: -30.0 },
    Shape::Ellipse { c: (0.32, 0.82), rx: 0.1, ry: 0.045, rot: 150.0 },
    Shape::Ellipse { c: (0.6, 0.52), rx: 0.07, ry: 0.035, rot: 20.0 },
    // A tendril curling off the stem.
    Shape::Curve {
        start: (0.5, 0.5),
        segs: &[[(0.35, 0.45), (0.25, 0.55), (0.3, 0.62)]],
        w: 0.015,
    },
];

const LEAVES: &[Shape] = &[
    Shape::Curve {
        start: (0.3, 0.7),
        segs: &[[(0.3, 0.4), (0.4, 0.3), (0.7, 0.3)], [(0.7, 0.6), (0.6, 0.7), (0.3, 0.7)]],
        w: 0.055,
    },
    Shape::Line { a: (0.3, 0.7), b: (0.62, 0.38), w: 0.045 },
    Shape::Line { a: (0.3, 0.7), b: (0.22, 0.78), w: 0.055 },
];

const DOTS: &[Shape] = &[Shape::Ellipse { c: (0.5, 0.5), rx: 0.14, ry: 0.14, rot: 0.0 }];

/// Each cell's diagonal meets the next one's, making long stripes.
const STRIPES: &[Shape] = &[Shape::Line { a: (-0.1, 1.1), b: (1.1, -0.1), w: 0.25 }];

const WAVES: &[Shape] = &[Shape::Curve {
    start: (0.0, 0.5),
    segs: &[[(0.17, 0.25), (0.33, 0.25), (0.5, 0.5)], [(0.67, 0.75), (0.83, 0.75), (1.0, 0.5)]],
    w: 0.07,
}];

const STARS: &[Shape] = &[
    Shape::Poly(&[
        (0.5, 0.3),
        (0.547, 0.435),
        (0.69, 0.438),
        (0.576, 0.525),
        (0.618, 0.662),
        (0.5, 0.58),
        (0.382, 0.662),
        (0.424, 0.525),
        (0.31, 0.438),
        (0.453, 0.435),
    ]),
    Shape::Ellipse { c: (0.0, 0.0), rx: 0.025, ry: 0.025, rot: 0.0 },
];

const HEARTS: &[Shape] = &[Shape::Curve {
    start: (0.5, 0.72),
    segs: &[[(0.2, 0.5), (0.28, 0.25), (0.5, 0.38)], [(0.72, 0.25), (0.8, 0.5), (0.5, 0.72)]],
    w: 0.0,
}];

/// Straight runs (a cubic with its control points on the ends is a line).
/// The main cracks leave each cell where the next one picks them up.
const CRACKS: &[Shape] = &[
    Shape::Curve {
        start: (0.0, 0.3),
        segs: &[
            [(0.0, 0.3), (0.35, 0.4), (0.35, 0.4)],
            [(0.35, 0.4), (0.55, 0.25), (0.55, 0.25)],
            [(0.55, 0.25), (1.0, 0.3), (1.0, 0.3)],
        ],
        w: 0.018,
    },
    Shape::Curve { start: (0.6, 0.0), segs: &[[(0.6, 0.0), (0.55, 0.25), (0.55, 0.25)]], w: 0.015 },
    Shape::Curve {
        start: (0.35, 0.4),
        segs: &[[(0.35, 0.4), (0.45, 0.7), (0.45, 0.7)], [(0.45, 0.7), (0.6, 1.0), (0.6, 1.0)]],
        w: 0.015,
    },
    Shape::Curve {
        start: (0.45, 0.7),
        segs: &[
            [(0.45, 0.7), (0.62, 0.78), (0.62, 0.78)],
            [(0.62, 0.78), (0.75, 0.74), (0.75, 0.74)],
        ],
        w: 0.01,
    },
    Shape::Curve {
        start: (0.55, 0.25),
        segs: &[
            [(0.55, 0.25), (0.7, 0.15), (0.7, 0.15)],
            [(0.7, 0.15), (0.78, 0.05), (0.78, 0.05)],
        ],
        w: 0.01,
    },
    Shape::Curve {
        start: (0.8, 0.29),
        segs: &[
            [(0.8, 0.29), (0.86, 0.45), (0.86, 0.45)],
            [(0.86, 0.45), (0.95, 0.55), (0.95, 0.55)],
        ],
        w: 0.008,
    },
    Shape::Line { a: (0.2, 0.35), b: (0.15, 0.55), w: 0.008 },
];

/// The repeating cell of `texture`; None for no texture.
pub fn texture_tile(texture: BgTexture) -> Option<Tile> {
    let (cell, stagger, shapes) = match texture {
        BgTexture::None => return None,
        BgTexture::Flowers => (34.0, true, FLOWERS),
        BgTexture::Vines => (40.0, false, VINES),
        BgTexture::Leaves => (30.0, true, LEAVES),
        BgTexture::Dots => (16.0, true, DOTS),
        BgTexture::Stripes => (14.0, false, STRIPES),
        BgTexture::Waves => (24.0, false, WAVES),
        BgTexture::Stars => (30.0, true, STARS),
        BgTexture::Hearts => (28.0, true, HEARTS),
        BgTexture::Cracks => (60.0, false, CRACKS),
    };
    Some(Tile { cell, stagger, shapes })
}

/// Most cells a scattered texture takes to repeat, across and down (even, so
/// staggered rows line up). Big textures repeat sooner, to keep the repeat small.
pub const SCATTER_PERIOD: i32 = 6;

/// Where a scattered texture puts cell (`col`, `row`)'s shapes: an offset in
/// cells (up to 0.3 each way) and a size (0.85–1.15). Repeats every `period`
/// cells, and differs with `seed` (one per panel).
pub fn scatter(seed: u32, col: i32, row: i32, period: i32) -> (f32, f32, f32) {
    let period = period.max(1);
    let (c, r) = (col.rem_euclid(period) as u32, row.rem_euclid(period) as u32);
    let mut h = seed ^ c.wrapping_mul(0x9e37_79b9) ^ r.wrapping_mul(0x85eb_ca6b);
    let mut next = || {
        // xorshift, then 0–1.
        h ^= h << 13;
        h ^= h >> 17;
        h ^= h << 5;
        (h & 0xffff) as f32 / 65535.0
    };
    next();
    let (dx, dy, k) = (next(), next(), next());
    ((dx - 0.5) * 0.6, (dy - 0.5) * 0.6, 0.85 + 0.3 * k)
}

/// A stable number for scattering one panel's texture its own way.
pub fn seed_of(id: &str) -> u32 {
    id.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193)) | 1
}

/// How far the blur effect reaches, in DIPs, at a strength of 0–1.
pub fn blur_radius(strength: f64) -> f32 {
    10.0 + 30.0 * strength.clamp(0.0, 1.0) as f32
}

/// How many times smaller to decode an image that gets blurred `radius_px`:
/// the blur hides the detail anyway, and a smaller picture costs less memory.
pub fn blur_reduce(radius_px: f32) -> u32 {
    ((radius_px / 4.0) as u32).clamp(1, 12)
}

/// Blurs premultiplied BGRA pixels, `w` x `h`, by about `radius` pixels:
/// three box blurs each way, which comes close to a Gaussian.
pub fn blur(px: &mut [u8], w: usize, h: usize, radius: f32) {
    let r = ((radius / 2.0).round() as usize).max(1);
    if w == 0 || h == 0 || px.len() < w * h * 4 {
        return;
    }
    let mut line = Vec::new();
    for _ in 0..3 {
        for y in 0..h {
            box_pass(px, y * w * 4, 4, w, r, &mut line);
        }
        for x in 0..w {
            box_pass(px, x * 4, w * 4, h, r, &mut line);
        }
    }
}

/// One box blur along a line of `n` pixels starting at byte `start`, `step`
/// bytes apart. Edge pixels repeat past the ends.
fn box_pass(px: &mut [u8], start: usize, step: usize, n: usize, r: usize, line: &mut Vec<[u8; 4]>) {
    line.clear();
    line.extend((0..n).map(|i| {
        let o = start + i * step;
        [px[o], px[o + 1], px[o + 2], px[o + 3]]
    }));
    let at = |i: isize| line[i.clamp(0, n as isize - 1) as usize];
    let width = (2 * r + 1) as u32;
    let mut sum = [0u32; 4];
    for i in -(r as isize)..=(r as isize) {
        let p = at(i);
        for c in 0..4 {
            sum[c] += p[c] as u32;
        }
    }
    for i in 0..n {
        let o = start + i * step;
        for c in 0..4 {
            px[o + c] = ((sum[c] + width / 2) / width) as u8;
        }
        let (add, drop) = (at(i as isize + r as isize + 1), at(i as isize - r as isize));
        for c in 0..4 {
            sum[c] = sum[c] + add[c] as u32 - drop[c] as u32;
        }
    }
}

/// How Windows places the wallpaper (the Personalization "fit" choices).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallFit {
    Center,
    Tile,
    Stretch,
    Fit,
    Fill,
    /// One picture across every monitor (`area` is all of them).
    Span,
}

/// Where a `src` sized wallpaper lands in `area` (a monitor, in screen pixels).
/// For `Tile`, the first tile.
pub fn wallpaper_rect(src: (u32, u32), area: Rect, fit: WallFit) -> Rect {
    let (sw, sh) = (src.0.max(1) as f32, src.1.max(1) as f32);
    let centered =
        |w: f32, h: f32| Rect::new(area.x + (area.w - w) / 2.0, area.y + (area.h - h) / 2.0, w, h);
    match fit {
        WallFit::Stretch => area,
        WallFit::Tile => Rect::new(area.x, area.y, sw, sh),
        WallFit::Center => centered(sw, sh),
        WallFit::Fit => {
            let k = (area.w / sw).min(area.h / sh);
            centered(sw * k, sh * k)
        }
        WallFit::Fill | WallFit::Span => {
            let k = (area.w / sw).max(area.h / sh);
            centered(sw * k, sh * k)
        }
    }
}

/// The part of a wallpaper that shows under a panel: which pixels of the
/// picture, and where they go in the panel (its pixels, from its corner).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallPlan {
    pub crop: (u32, u32, u32, u32),
    pub dest: Rect,
}

/// Plans cutting the wallpaper under `panel` (screen pixels), for any fit
/// but `Tile`. None when none of it shows there.
pub fn wallpaper_plan(src: (u32, u32), area: Rect, panel: Rect, fit: WallFit) -> Option<WallPlan> {
    let placed = wallpaper_rect(src, area, fit);
    let x0 = placed.x.max(panel.x).max(area.x);
    let y0 = placed.y.max(panel.y).max(area.y);
    let x1 = placed.right().min(panel.right()).min(area.right());
    let y1 = placed.bottom().min(panel.bottom()).min(area.bottom());
    if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        return None;
    }
    let (kx, ky) = (placed.w / src.0.max(1) as f32, placed.h / src.1.max(1) as f32);
    let cx = (((x0 - placed.x) / kx).floor() as u32).min(src.0.saturating_sub(1));
    let cy = (((y0 - placed.y) / ky).floor() as u32).min(src.1.saturating_sub(1));
    let cw = (((x1 - placed.x) / kx).ceil() as u32).min(src.0).saturating_sub(cx).max(1);
    let ch = (((y1 - placed.y) / ky).ceil() as u32).min(src.1).saturating_sub(cy).max(1);
    Some(WallPlan {
        crop: (cx, cy, cw, ch),
        dest: Rect::new(x0 - panel.x, y0 - panel.y, x1 - x0, y1 - y0),
    })
}

/// Top-left corners of the cells that cover `r`, counted from its corner so
/// the pattern moves with the panel. One extra cell around the edges, since
/// shapes may reach a little out of their cell.
pub fn tile_origins(r: Rect, tile: &Tile) -> Vec<(f32, f32)> {
    let c = tile.cell;
    let rows = (r.h / c).ceil() as i32 + 1;
    let cols = (r.w / c).ceil() as i32 + 1;
    let mut out = Vec::with_capacity(((rows + 1) * (cols + 2)).max(0) as usize);
    for row in -1..rows {
        let shift = if tile.stagger && row.rem_euclid(2) == 1 { -c / 2.0 } else { 0.0 };
        for col in -1..=cols {
            out.push((r.x + col as f32 * c + shift, r.y + row as f32 * c));
        }
    }
    out
}

/// Where an image `w` x `h` goes in `dest`. `Tile` gives the first tile, at
/// the image's own size.
pub fn image_rect(w: f32, h: f32, dest: Rect, fit: ImageFit) -> Rect {
    let (w, h) = (w.max(1.0), h.max(1.0));
    let k = match fit {
        ImageFit::Stretch => return dest,
        ImageFit::Tile => return Rect::new(dest.x, dest.y, w, h),
        ImageFit::Fill => (dest.w / w).max(dest.h / h),
        ImageFit::Fit => (dest.w / w).min(dest.h / h),
    };
    let (sw, sh) = (w * k, h * k);
    Rect::new(dest.x + (dest.w - sw) / 2.0, dest.y + (dest.h - sh) / 2.0, sw, sh)
}

/// Largest side of a decoded background image, in pixels.
pub const MAX_IMAGE_SIDE: u32 = 2048;

/// How to decode an image for a panel: which part of it, at what size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecodePlan {
    /// The part of the image to keep, in its pixels: x, y, width, height.
    pub crop: (u32, u32, u32, u32),
    /// The decoded size in pixels.
    pub size: (u32, u32),
    /// How many DIPs one decoded pixel covers when tiled (shown at the image's own size).
    pub dip_per_px: f32,
}

/// Plans decoding a `src` sized image for a panel `target` pixels big (its
/// DIP size times `scale`), keeping only pixels that show: `Fill` crops to
/// the panel's shape, nothing is made bigger than the image itself (drawing
/// stretches it for free), and no side passes `MAX_IMAGE_SIDE`.
pub fn decode_plan(src: (u32, u32), target: (u32, u32), fit: ImageFit, scale: f32) -> DecodePlan {
    let (sw, sh) = (src.0.max(1), src.1.max(1));
    let (tw, th) = (target.0.max(1) as f32, target.1.max(1) as f32);
    let mut crop = (0, 0, sw, sh);
    if fit == ImageFit::Fill {
        let (fw, fh) = (sw as f32, sh as f32);
        if fw / fh > tw / th {
            let w = ((fh * tw / th).round() as u32).clamp(1, sw);
            crop = ((sw - w) / 2, 0, w, sh);
        } else {
            let h = ((fw * th / tw).round() as u32).clamp(1, sh);
            crop = (0, (sh - h) / 2, sw, h);
        }
    }
    let (cw, ch) = (crop.2 as f32, crop.3 as f32);
    let k = match fit {
        // Shown one image pixel per DIP.
        ImageFit::Tile => scale,
        ImageFit::Fill => tw / cw,
        ImageFit::Fit => (tw / cw).min(th / ch),
        // Stretched to the panel, so each side on its own; see below.
        ImageFit::Stretch => 1.0,
    };
    let (mut w, mut h) = if fit == ImageFit::Stretch {
        (tw.min(cw), th.min(ch))
    } else {
        let k = k.min(1.0);
        (cw * k, ch * k)
    };
    let cap = MAX_IMAGE_SIDE as f32 / w.max(h);
    if cap < 1.0 {
        (w, h) = (w * cap, h * cap);
    }
    let size = ((w.round() as u32).max(1), (h.round() as u32).max(1));
    DecodePlan { crop, size, dip_per_px: cw / size.0 as f32 }
}

/// The size of a pixelated block, in DIPs, at an effect strength of 0–1.
pub fn pixel_block(strength: f64) -> f32 {
    3.0 + 13.0 * strength.clamp(0.0, 1.0) as f32
}

/// Side of the grain effect's square noise tile, in pixels.
pub const GRAIN_SIZE: u32 = 64;

/// The grain effect's noise: light and dark specks of varying strength, as
/// premultiplied BGRA, `GRAIN_SIZE` square. The same every time.
pub fn grain_tile() -> Vec<u8> {
    let n = (GRAIN_SIZE * GRAIN_SIZE) as usize;
    let mut out = Vec::with_capacity(n * 4);
    let mut h: u32 = 0x9e37_79b9;
    for _ in 0..n {
        // xorshift: cheap, and plenty random for noise.
        h ^= h << 13;
        h ^= h >> 17;
        h ^= h << 5;
        let a = (h & 0xff) * 64 / 255;
        let v = if h & 0x100 != 0 { a } else { 0 };
        let (v, a) = (v as u8, a as u8);
        out.extend_from_slice(&[v, v, v, a]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_texture_but_none_has_shapes_near_its_cell() {
        assert_eq!(texture_tile(BgTexture::None), None);
        for t in BgTexture::ALL.into_iter().skip(1) {
            let tile = texture_tile(t).unwrap();
            assert!(tile.cell >= 10.0 && !tile.shapes.is_empty(), "{t:?}");
            let near = |p: &Pt| (-0.2..=1.2).contains(&p.0) && (-0.2..=1.2).contains(&p.1);
            for s in tile.shapes {
                let ok = match s {
                    Shape::Ellipse { c, .. } => near(c),
                    Shape::Line { a, b, .. } => near(a) && near(b),
                    Shape::Curve { start, segs, .. } => {
                        near(start) && segs.iter().flatten().all(near)
                    }
                    Shape::Poly(pts) => pts.len() >= 3 && pts.iter().all(near),
                };
                assert!(ok, "{t:?}: {s:?}");
            }
        }
    }

    #[test]
    fn vine_stems_join_from_cell_to_cell() {
        let Shape::Curve { start, segs, .. } = VINES[0] else { panic!() };
        let [c1, c2, end] = segs[0];
        assert_eq!((start.0, end.0), (0.5, 0.5));
        assert_eq!((start.1, end.1), (0.0, 1.0));
        // Same direction out of the bottom as into the top.
        assert_eq!((c1.0 - start.0, c1.1 - start.1), (end.0 - c2.0, end.1 - c2.1));
    }

    #[test]
    fn tiles_cover_the_whole_rect() {
        let r = Rect::new(100.0, 50.0, 95.0, 61.0);
        for t in [BgTexture::Dots, BgTexture::Vines] {
            let tile = texture_tile(t).unwrap();
            let cells = tile_origins(r, &tile);
            let c = tile.cell;
            for (x, y) in
                [(r.x, r.y), (r.right() - 0.1, r.bottom() - 0.1), (r.x + 40.0, r.y + 30.0)]
            {
                assert!(
                    cells.iter().any(|&(cx, cy)| cx <= x && x < cx + c && cy <= y && y < cy + c),
                    "{t:?} at {x},{y}"
                );
            }
            // Anchored to the rect's corner.
            assert!(cells.contains(&(r.x, r.y)));
        }
    }

    #[test]
    fn staggers_odd_rows_by_half_a_cell() {
        let tile = texture_tile(BgTexture::Dots).unwrap();
        let cells = tile_origins(Rect::new(0.0, 0.0, 32.0, 32.0), &tile);
        assert!(cells.contains(&(0.0, 0.0)));
        assert!(cells.contains(&(-8.0, 16.0)));
    }

    #[test]
    fn places_images_by_fit() {
        let dest = Rect::new(10.0, 20.0, 200.0, 100.0);
        // A square image: fill covers the width, fit the height.
        assert_eq!(
            image_rect(50.0, 50.0, dest, ImageFit::Fill),
            Rect::new(10.0, -30.0, 200.0, 200.0)
        );
        assert_eq!(
            image_rect(50.0, 50.0, dest, ImageFit::Fit),
            Rect::new(60.0, 20.0, 100.0, 100.0)
        );
        assert_eq!(image_rect(50.0, 50.0, dest, ImageFit::Stretch), dest);
        assert_eq!(image_rect(50.0, 40.0, dest, ImageFit::Tile), Rect::new(10.0, 20.0, 50.0, 40.0));
        // A zero-sized image doesn't divide by zero.
        assert!(image_rect(0.0, 0.0, dest, ImageFit::Fill).w.is_finite());
    }

    #[test]
    fn fill_crops_to_the_panel_shape_and_scales_down() {
        // A wide photo for a 600 x 400 px panel: the middle 1500 x 1000 is kept.
        let p = decode_plan((3000, 1000), (600, 400), ImageFit::Fill, 1.5);
        assert_eq!(p.crop, (750, 0, 1500, 1000));
        assert_eq!(p.size, (600, 400));
        // A tall one keeps the middle band.
        let p = decode_plan((1000, 3000), (600, 400), ImageFit::Fill, 1.0);
        assert_eq!(p.crop, (0, 1166, 1000, 667));
        assert_eq!(p.size, (600, 400));
    }

    #[test]
    fn never_decodes_bigger_than_the_image() {
        let p = decode_plan((100, 50), (600, 400), ImageFit::Fill, 1.0);
        assert_eq!(p.crop, (12, 0, 75, 50));
        assert_eq!(p.size, (75, 50));
        assert_eq!(decode_plan((100, 50), (600, 400), ImageFit::Fit, 1.0).size, (100, 50));
        assert_eq!(decode_plan((100, 50), (600, 400), ImageFit::Stretch, 1.0).size, (100, 50));
    }

    #[test]
    fn fit_and_stretch_keep_the_whole_image() {
        let p = decode_plan((2000, 1000), (600, 400), ImageFit::Fit, 1.0);
        assert_eq!((p.crop, p.size), ((0, 0, 2000, 1000), (600, 300)));
        let p = decode_plan((2000, 1000), (600, 400), ImageFit::Stretch, 1.0);
        assert_eq!((p.crop, p.size), ((0, 0, 2000, 1000), (600, 400)));
    }

    #[test]
    fn tiles_at_the_image_size_and_caps_huge_images() {
        let p = decode_plan((200, 100), (600, 400), ImageFit::Tile, 1.5);
        // Not made bigger for the display scale; each pixel covers one DIP.
        assert_eq!((p.size, p.dip_per_px), ((200, 100), 1.0));
        let p = decode_plan((8000, 4000), (600, 400), ImageFit::Tile, 1.0);
        assert_eq!(p.size, (2048, 1024));
        assert!((p.dip_per_px - 8000.0 / 2048.0).abs() < 1e-4);
    }

    #[test]
    fn cracks_join_from_cell_to_cell() {
        let ends: Vec<Pt> = CRACKS
            .iter()
            .flat_map(|s| match s {
                Shape::Curve { start, segs, .. } => vec![*start, segs.last().unwrap()[2]],
                _ => vec![],
            })
            .collect();
        // What leaves on the right comes in on the left, and the bottom meets the top.
        for (out, into) in [((1.0, 0.3), (0.0, 0.3)), ((0.6, 1.0), (0.6, 0.0))] {
            assert!(ends.contains(&out) && ends.contains(&into));
        }
    }

    #[test]
    fn scatter_stays_close_and_repeats() {
        let seed = seed_of("panel-a");
        for col in -8..8 {
            for row in -8..8 {
                let (dx, dy, k) = scatter(seed, col, row, 6);
                assert!(dx.abs() <= 0.3 && dy.abs() <= 0.3, "{dx} {dy}");
                assert!((0.85..=1.15).contains(&k), "{k}");
                assert_eq!(scatter(seed, col, row, 6), scatter(seed, col + 6, row, 6));
                assert_eq!(scatter(seed, col, row, 4), scatter(seed, col, row - 4, 4));
            }
        }
        assert_ne!(scatter(seed, 0, 0, 6), scatter(seed, 1, 0, 6));
        assert_ne!(scatter(seed, 0, 0, 6), scatter(seed_of("panel-b"), 0, 0, 6));
    }

    #[test]
    fn blur_keeps_flat_color_and_spreads_a_dot() {
        let (w, h) = (9, 7);
        let mut flat: Vec<u8> = [10, 20, 30, 255].repeat(w * h);
        blur(&mut flat, w, h, 6.0);
        assert!(flat.chunks(4).all(|p| p == [10, 20, 30, 255]));

        let mut dot = vec![0u8; w * h * 4];
        let mid = (3 * w + 4) * 4;
        dot[mid..mid + 4].copy_from_slice(&[255, 255, 255, 255]);
        blur(&mut dot, w, h, 2.0);
        assert!(dot[mid + 3] < 255 && dot[mid + 3] > 0);
        assert!(dot[(3 * w + 5) * 4 + 3] > 0, "reaches the neighbour");
        assert!(dot.chunks(4).all(|p| p[0] <= p[3]), "stays premultiplied");
    }

    #[test]
    fn blur_reduces_more_for_bigger_blurs() {
        assert_eq!(blur_reduce(2.0), 1);
        assert_eq!(blur_reduce(20.0), 5);
        assert_eq!(blur_reduce(400.0), 12);
        assert!(blur_radius(1.0) > blur_radius(0.0));
    }

    #[test]
    fn places_the_wallpaper_like_windows() {
        let screen = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        // A 4:3 picture filling a 16:9 screen is cropped top and bottom.
        assert_eq!(
            wallpaper_rect((960, 720), screen, WallFit::Fill),
            Rect::new(0.0, -180.0, 1920.0, 1440.0)
        );
        assert_eq!(
            wallpaper_rect((960, 720), screen, WallFit::Fit),
            Rect::new(240.0, 0.0, 1440.0, 1080.0)
        );
        assert_eq!(
            wallpaper_rect((800, 600), screen, WallFit::Center),
            Rect::new(560.0, 240.0, 800.0, 600.0)
        );
        assert_eq!(wallpaper_rect((800, 600), screen, WallFit::Stretch), screen);
    }

    #[test]
    fn cuts_the_wallpaper_under_a_panel() {
        let screen = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        // Stretched 2x: a panel at (100, 50) sees the picture from (50, 25).
        let p = wallpaper_plan(
            (960, 540),
            screen,
            Rect::new(100.0, 50.0, 400.0, 300.0),
            WallFit::Stretch,
        )
        .unwrap();
        assert_eq!(p.crop, (50, 25, 200, 150));
        assert_eq!(p.dest, Rect::new(0.0, 0.0, 400.0, 300.0));
        // Centered small picture: the panel's top left corner is beside it.
        let p = wallpaper_plan(
            (800, 600),
            screen,
            Rect::new(500.0, 200.0, 200.0, 100.0),
            WallFit::Center,
        )
        .unwrap();
        assert_eq!(p.crop, (0, 0, 140, 60));
        assert_eq!(p.dest, Rect::new(60.0, 40.0, 140.0, 60.0));
        // A panel past the picture sees none of it.
        assert_eq!(
            wallpaper_plan((800, 600), screen, Rect::new(0.0, 0.0, 100.0, 100.0), WallFit::Center),
            None
        );
    }

    #[test]
    fn pixel_blocks_grow_with_strength() {
        assert_eq!(pixel_block(0.0), 3.0);
        assert_eq!(pixel_block(1.0), 16.0);
        assert_eq!(pixel_block(7.0), 16.0);
        assert!(pixel_block(0.5) > pixel_block(0.2));
    }

    #[test]
    fn grain_is_premultiplied_and_stable() {
        let g = grain_tile();
        assert_eq!(g.len(), (GRAIN_SIZE * GRAIN_SIZE * 4) as usize);
        assert_eq!(g, grain_tile());
        let px = g.as_chunks::<4>().0;
        assert!(px.iter().all(|p| p[0] <= p[3] && p[0] == p[1] && p[1] == p[2]));
        assert!(px.iter().any(|p| p[0] > 0) && px.iter().any(|p| p[0] == 0 && p[3] > 0));
    }
}
