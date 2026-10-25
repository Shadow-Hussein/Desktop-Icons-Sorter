//! Drawing with Direct2D and DirectWrite in software, into a DIB that is
//! handed to `UpdateLayeredWindow` for per-pixel alpha. No GPU is involved,
//! so there's no GPU process or driver memory to pay for.
//!
//! All windows share one render target and one DIB (grown to the largest
//! window drawn), since only one window is drawn at a time. `UpdateLayeredWindow`
//! keeps its own copy of the pixels, so nothing needs to stay per window.

use std::ffi::c_void;
use std::mem::ManuallyDrop;

use windows::Win32::Graphics::Direct2D::{
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, ID2D1Brush, ID2D1LinearGradientBrush,
};
use windows::core::Interface;

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_BITMAP_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_EXTEND_MODE_CLAMP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_GAMMA_2_2,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget,
    ID2D1Factory, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE,
    DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_EMERGENCY_BREAK,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
    IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};
use windows::core::{Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use super::utf16;
use sorter::panel_layout::Rect;

/// Straight-alpha color, 0–1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub f32, pub f32, pub f32, pub f32);

impl Rgba {
    pub const fn white(a: f32) -> Self {
        Self(1.0, 1.0, 1.0, a)
    }

    pub const fn black(a: f32) -> Self {
        Self(0.0, 0.0, 0.0, a)
    }

    pub const ACCENT: Self =
        Self(0x2e as f32 / 255.0, 0x9c as f32 / 255.0, 0xf0 as f32 / 255.0, 1.0);

    /// `#rrggbb` with an opacity; white when the hex is malformed (config validation prevents that).
    pub fn hex(hex: &str, a: f32) -> Self {
        let c = |i: usize| {
            hex.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(255) as f32
                / 255.0
        };
        Self(c(1), c(3), c(5), a)
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self(self.0, self.1, self.2, a)
    }

    fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: self.0, g: self.1, b: self.2, a: self.3 }
    }
}

fn rect_f(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x, top: r.y, right: r.right(), bottom: r.bottom() }
}

/// Text styles used across the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    /// Panel titles: 12.5 px semibold, one line, centered.
    Title,
    /// Icon names: 11.5 px, wrapping, centered.
    Label,
    /// Hints like "Drag icons here": 12 px italic, centered.
    Hint,
    /// Settings window body text: 12 px, left aligned.
    Body,
    /// Settings section headings: 12 px semibold, left aligned.
    Heading,
    /// Settings buttons and segmented controls: 12 px, centered on one line.
    Button,
}

struct Formats {
    title: IDWriteTextFormat,
    label: IDWriteTextFormat,
    hint: IDWriteTextFormat,
    body: IDWriteTextFormat,
    heading: IDWriteTextFormat,
    button: IDWriteTextFormat,
}

/// A DIB section selected into a memory DC, for the render target to draw into.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    w: i32,
    h: i32,
}

impl Surface {
    fn new(w: i32, h: i32) -> Result<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .inspect_err(|_| {
                    let _ = DeleteDC(dc);
                })?;
            let old = SelectObject(dc, bitmap.into());
            Ok(Self { dc, bitmap, old, w, h })
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

pub struct Gfx {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    formats: Formats,
    surface: Option<Surface>,
    /// Bumped when the render target is recreated, so bitmaps made for the old one are dropped.
    pub generation: u32,
}

fn format(
    dw: &IDWriteFactory,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
    style: DWRITE_FONT_STYLE,
    align: DWRITE_TEXT_ALIGNMENT,
    wrap: bool,
) -> Result<IDWriteTextFormat> {
    unsafe {
        let f = dw.CreateTextFormat(
            w!("Segoe UI"),
            None,
            weight,
            style,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!(""),
        )?;
        f.SetTextAlignment(align)?;
        if wrap {
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_EMERGENCY_BREAK)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
        } else {
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        }
        let sign = dw.CreateEllipsisTrimmingSign(&f)?;
        let trimming = DWRITE_TRIMMING {
            granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            delimiter: 0,
            delimiterCount: 0,
        };
        f.SetTrimming(&trimming, &sign)?;
        Ok(f)
    }
}

fn create_target(d2d: &ID2D1Factory) -> Result<(ID2D1DCRenderTarget, ID2D1SolidColorBrush)> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    unsafe {
        let rt = d2d.CreateDCRenderTarget(&props)?;
        // ClearType needs an opaque background; these windows are transparent.
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let brush = rt.CreateSolidColorBrush(&Rgba::white(1.0).d2d(), None)?;
        Ok((rt, brush))
    }
}

impl Gfx {
    pub fn new() -> Result<Self> {
        let d2d: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let (n, c, l) =
            (DWRITE_FONT_STYLE_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING);
        let formats = Formats {
            title: format(&dwrite, 12.5, DWRITE_FONT_WEIGHT_SEMI_BOLD, n, c, false)?,
            label: format(&dwrite, 11.5, DWRITE_FONT_WEIGHT_NORMAL, n, c, true)?,
            hint: format(
                &dwrite,
                12.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_ITALIC,
                c,
                true,
            )?,
            body: format(&dwrite, 12.0, DWRITE_FONT_WEIGHT_NORMAL, n, l, true)?,
            heading: format(&dwrite, 12.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, n, l, false)?,
            button: format(&dwrite, 12.0, DWRITE_FONT_WEIGHT_NORMAL, n, c, false)?,
        };
        let (rt, brush) = create_target(&d2d)?;
        Ok(Self { d2d, dwrite, rt, brush, formats, surface: None, generation: 0 })
    }

    fn text_format(&self, font: Font) -> &IDWriteTextFormat {
        match font {
            Font::Title => &self.formats.title,
            Font::Label => &self.formats.label,
            Font::Hint => &self.formats.hint,
            Font::Body => &self.formats.body,
            Font::Heading => &self.formats.heading,
            Font::Button => &self.formats.button,
        }
    }

    /// How wide and tall `text` is in `font`, laid out at most `max_w` wide.
    pub fn measure(&self, text: &str, font: Font, max_w: f32) -> (f32, f32) {
        let layout = self.layout(text, font, max_w, f32::MAX);
        let mut m = DWRITE_TEXT_METRICS::default();
        match layout.and_then(|l| unsafe { l.GetMetrics(&mut m) }) {
            Ok(()) => (m.widthIncludingTrailingWhitespace, m.height),
            Err(_) => (0.0, 0.0),
        }
    }

    fn layout(&self, text: &str, font: Font, w: f32, h: f32) -> Result<IDWriteTextLayout> {
        unsafe { self.dwrite.CreateTextLayout(&utf16(text), self.text_format(font), w, h) }
    }

    /// Makes a bitmap from top-down premultiplied BGRA pixels.
    pub fn bitmap(&self, w: u32, h: u32, pixels: &[u8]) -> Result<ID2D1Bitmap> {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        unsafe {
            self.rt.CreateBitmap(
                D2D_SIZE_U { width: w, height: h },
                Some(pixels.as_ptr() as *const c_void),
                w * 4,
                &props,
            )
        }
    }

    fn surface(&mut self, w: i32, h: i32) -> Result<&Surface> {
        let fits = self.surface.as_ref().is_some_and(|s| s.w >= w && s.h >= h);
        if !fits {
            let (ow, oh) = self.surface.as_ref().map_or((0, 0), |s| (s.w, s.h));
            self.surface = None;
            self.surface = Some(Surface::new(w.max(ow), h.max(oh))?);
        }
        Ok(self.surface.as_ref().expect("just made"))
    }

    /// Lets go of the drawing surface, e.g. after a large window got smaller.
    pub fn trim(&mut self) {
        self.surface = None;
    }

    /// Draws a window `rect` (physical pixels, screen position and size) at
    /// `scale`, then shows it with per-pixel alpha. `draw` works in DIPs.
    pub fn present(
        &mut self,
        hwnd: HWND,
        rect: RECT,
        scale: f32,
        draw: impl FnOnce(&mut Canvas),
    ) -> Result<()> {
        let (w, h) = ((rect.right - rect.left).max(1), (rect.bottom - rect.top).max(1));
        let dc = self.surface(w, h)?.dc;
        let bind = RECT { left: 0, top: 0, right: w, bottom: h };
        unsafe {
            self.rt.BindDC(dc, &bind)?;
            self.rt.SetDpi(96.0 * scale, 96.0 * scale);
            self.rt.BeginDraw();
            self.rt.SetTransform(&Matrix3x2::identity());
            self.rt.Clear(Some(&Rgba::black(0.0).d2d()));
        }
        let mut canvas = Canvas { gfx: self, scale };
        draw(&mut canvas);
        if let Err(e) = unsafe { self.rt.EndDraw(None, None) } {
            // The target is lost only in rare cases (D2DERR_RECREATE_TARGET); start over.
            let (rt, brush) = create_target(&self.d2d)?;
            (self.rt, self.brush) = (rt, brush);
            self.generation += 1;
            return Err(e);
        }
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let pos = POINT { x: rect.left, y: rect.top };
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&pos),
                Some(&size),
                Some(dc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }

    /// Draws into a normal (opaque) window's DC, for the settings windows.
    pub fn paint_dc(
        &mut self,
        hdc: HDC,
        w: i32,
        h: i32,
        scale: f32,
        draw: impl FnOnce(&mut Canvas),
    ) -> Result<()> {
        let bind = RECT { left: 0, top: 0, right: w.max(1), bottom: h.max(1) };
        unsafe {
            self.rt.BindDC(hdc, &bind)?;
            self.rt.SetDpi(96.0 * scale, 96.0 * scale);
            self.rt.BeginDraw();
            self.rt.SetTransform(&Matrix3x2::identity());
        }
        let mut canvas = Canvas { gfx: self, scale };
        draw(&mut canvas);
        let r = unsafe { self.rt.EndDraw(None, None) };
        if r.is_err() {
            let (rt, brush) = create_target(&self.d2d)?;
            (self.rt, self.brush) = (rt, brush);
            self.generation += 1;
        }
        r
    }
}

/// Drawing calls for one frame, in DIPs.
pub struct Canvas<'a> {
    gfx: &'a Gfx,
    pub scale: f32,
}

impl Canvas<'_> {
    pub fn gfx(&self) -> &Gfx {
        self.gfx
    }

    fn brush(&self, c: Rgba) -> &ID2D1SolidColorBrush {
        unsafe { self.gfx.brush.SetColor(&c.d2d()) };
        &self.gfx.brush
    }

    pub fn fill_rect(&mut self, r: Rect, c: Rgba) {
        unsafe { self.gfx.rt.FillRectangle(&rect_f(r), self.brush(c)) };
    }

    pub fn fill_round(&mut self, r: Rect, radius: f32, c: Rgba) {
        let rr = D2D1_ROUNDED_RECT { rect: rect_f(r), radiusX: radius, radiusY: radius };
        unsafe { self.gfx.rt.FillRoundedRectangle(&rr, self.brush(c)) };
    }

    /// A rounded outline drawn inside `r` (the stroke doesn't spill out of it).
    pub fn stroke_round(&mut self, r: Rect, radius: f32, c: Rgba, width: f32) {
        let r = r.inset(width / 2.0);
        let rr = D2D1_ROUNDED_RECT { rect: rect_f(r), radiusX: radius, radiusY: radius };
        unsafe { self.gfx.rt.DrawRoundedRectangle(&rr, self.brush(c), width, None) };
    }

    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), c: Rgba, width: f32) {
        let (a, b) = (Vector2 { X: from.0, Y: from.1 }, Vector2 { X: to.0, Y: to.1 });
        unsafe { self.gfx.rt.DrawLine(a, b, self.brush(c), width, None) };
    }

    pub fn fill_ellipse(&mut self, center: (f32, f32), rx: f32, ry: f32, c: Rgba) {
        let e = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
            point: Vector2 { X: center.0, Y: center.1 },
            radiusX: rx,
            radiusY: ry,
        };
        unsafe { self.gfx.rt.FillEllipse(&e, self.brush(c)) };
    }

    pub fn stroke_ellipse(&mut self, center: (f32, f32), rx: f32, ry: f32, c: Rgba, width: f32) {
        let e = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
            point: Vector2 { X: center.0, Y: center.1 },
            radiusX: rx,
            radiusY: ry,
        };
        unsafe { self.gfx.rt.DrawEllipse(&e, self.brush(c), width, None) };
    }

    fn gradient(
        &self,
        from: (f32, f32),
        to: (f32, f32),
        stops: &[(f32, Rgba)],
    ) -> Option<ID2D1LinearGradientBrush> {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(position, c)| D2D1_GRADIENT_STOP { position, color: c.d2d() })
            .collect();
        let rt = &self.gfx.rt;
        unsafe {
            let collection = rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                .ok()?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 { X: from.0, Y: from.1 },
                endPoint: Vector2 { X: to.0, Y: to.1 },
            };
            rt.CreateLinearGradientBrush(&props, None, &collection).ok()
        }
    }

    /// Fills `r` with a linear gradient from `from` to `to` through `stops` (offset, color).
    pub fn fill_gradient(
        &mut self,
        r: Rect,
        radius: f32,
        from: (f32, f32),
        to: (f32, f32),
        stops: &[(f32, Rgba)],
    ) {
        let Some(brush) = self.gradient(from, to, stops) else { return };
        let rr = D2D1_ROUNDED_RECT { rect: rect_f(r), radiusX: radius, radiusY: radius };
        unsafe { self.gfx.rt.FillRoundedRectangle(&rr, &brush) };
    }

    /// Fills `r` with a gradient (`from` → `to` through `stops`), faded by a
    /// second gradient of opacities (`mask_from` → `mask_to` through `mask`).
    /// Together they fade a rectangle out on all four sides.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_masked_gradient(
        &mut self,
        r: Rect,
        from: (f32, f32),
        to: (f32, f32),
        stops: &[(f32, Rgba)],
        mask_from: (f32, f32),
        mask_to: (f32, f32),
        mask: &[(f32, f32)],
    ) {
        let mask: Vec<(f32, Rgba)> = mask.iter().map(|&(o, a)| (o, Rgba::black(a))).collect();
        let (Some(brush), Some(opacity)) =
            (self.gradient(from, to, stops), self.gradient(mask_from, mask_to, &mask))
        else {
            return;
        };
        let rt = &self.gfx.rt;
        unsafe {
            let Ok(layer) = rt.CreateLayer(None) else { return };
            let Ok(opacity) = opacity.cast::<ID2D1Brush>() else { return };
            let params = D2D1_LAYER_PARAMETERS {
                contentBounds: rect_f(r),
                geometricMask: ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: ManuallyDrop::new(Some(opacity)),
                layerOptions: D2D1_LAYER_OPTIONS_NONE,
            };
            rt.PushLayer(&params, &layer);
            rt.FillRectangle(&rect_f(r), &brush);
            rt.PopLayer();
            drop(ManuallyDrop::into_inner(params.opacityBrush));
        }
    }

    /// Draws `text` in `r`, trimmed with an ellipsis when it doesn't fit.
    pub fn text(&mut self, text: &str, font: Font, r: Rect, c: Rgba) {
        let s = utf16(text);
        let f = self.gfx.text_format(font);
        unsafe {
            self.gfx.rt.DrawText(
                &s,
                f,
                &rect_f(r),
                self.brush(c),
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
    }

    /// Text with a soft dark shadow under it, readable on any wallpaper.
    pub fn text_shadowed(&mut self, text: &str, font: Font, r: Rect, c: Rgba, shadow: f32) {
        let off = |dx: f32, dy: f32| Rect::new(r.x + dx, r.y + dy, r.w, r.h);
        self.text(text, font, off(0.0, 1.0), Rgba::black(shadow * 0.7));
        self.text(text, font, off(-0.6, 0.6), Rgba::black(shadow * 0.25));
        self.text(text, font, off(0.6, 0.6), Rgba::black(shadow * 0.25));
        self.text(text, font, r, c);
    }

    pub fn bitmap(&mut self, b: &ID2D1Bitmap, r: Rect, opacity: f32) {
        unsafe {
            self.gfx.rt.DrawBitmap(
                b,
                Some(&rect_f(r)),
                opacity,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            )
        };
    }

    pub fn clip(&mut self, r: Rect) {
        unsafe { self.gfx.rt.PushAxisAlignedClip(&rect_f(r), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    }

    pub fn unclip(&mut self) {
        unsafe { self.gfx.rt.PopAxisAlignedClip() };
    }

    /// Rotates what's drawn next by `degrees` around `center`; `reset` undoes it.
    pub fn rotate(&mut self, degrees: f32, center: (f32, f32)) {
        let m = Matrix3x2::rotation_around(degrees, Vector2 { X: center.0, Y: center.1 });
        unsafe { self.gfx.rt.SetTransform(&m) };
    }

    pub fn reset(&mut self) {
        unsafe { self.gfx.rt.SetTransform(&Matrix3x2::identity()) };
    }
}
