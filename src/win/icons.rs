//! Item icons, as Explorer shows them, and a small LRU cache of them.
//!
//! Order of preference, as in the Electron app: the real icon a shortcut or
//! .url points at, extracted at the exact size (Explorer-quality icons for
//! Steam games and the like); then the shell's own image for the item, which
//! covers thumbnails, folders and documents.

use std::ffi::c_void;
use std::path::Path;

use sorter::icon_source::{IconSource, IconSourceDeps, ShortcutDetails, resolve_icon_source};
use sorter::overlay::{
    SHELL_ICONS_KEY, SHORTCUT_OVERLAY_VALUE, default_arrow, parse_icon_location,
};
use windows::Win32::Foundation::{MAX_PATH, SIZE};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, HDC, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RegGetValueW,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, IShellLinkW, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetIconInfo, HICON, ICONINFO, PrivateExtractIconsW,
};
use windows::core::Interface;

use super::{Wide, from_wide, render::Gfx};

/// Top-down BGRA with premultiplied alpha.
pub struct Pixels {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

/// Reads a bitmap as top-down 32-bit BGRA.
fn read_bitmap(hdc: HDC, bitmap: HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = vec![0u8; (w * h * 4) as usize];
    let lines = unsafe {
        GetDIBits(
            hdc,
            bitmap,
            0,
            h as u32,
            Some(bits.as_mut_ptr() as *mut c_void),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    (lines == h).then_some(bits)
}

/// Premultiplied pixels never have a color channel above their alpha.
fn is_straight_alpha(px: &[u8]) -> bool {
    px.as_chunks::<4>().0.iter().any(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3])
}

fn has_alpha(px: &[u8]) -> bool {
    px.as_chunks::<4>().0.iter().any(|p| p[3] != 0)
}

fn premultiply(px: &mut [u8]) {
    for p in px.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a != 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * a + 127) / 255) as u8;
            }
        }
    }
}

/// Icon `index` (or resource ID, when negative) from a file with icon
/// resources, at `size` x `size`.
pub fn extract_icon(file: &str, index: i32, size: i32) -> Option<Pixels> {
    let mut path = [0u16; MAX_PATH as usize];
    let w = Wide::new(file);
    let n = w.chars().len().min(path.len() - 1);
    path[..n].copy_from_slice(&w.chars()[..n]);
    let mut icons = [HICON::default()];
    let mut ids = [0u32];
    let count = unsafe {
        PrivateExtractIconsW(&path, index, size, size, Some(&mut icons), Some(ids.as_mut_ptr()), 0)
    };
    // 0 = none found; 0xFFFFFFFF = file not found or not an icon container.
    if count == 0 || count == u32::MAX || icons[0].is_invalid() {
        return None;
    }
    let icon = icons[0];
    let out = unsafe { icon_pixels(icon, size) };
    unsafe {
        let _ = DestroyIcon(icon);
    }
    out
}

unsafe fn icon_pixels(icon: HICON, size: i32) -> Option<Pixels> {
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    let hdc = unsafe { GetDC(None) };
    let result = (|| {
        // Monochrome icons have no color bitmap; let the caller fall back.
        if info.hbmColor.is_invalid() {
            return None;
        }
        let mut color = read_bitmap(hdc, info.hbmColor, size, size)?;
        if has_alpha(&color) {
            // Icon bitmaps hold straight alpha; Direct2D wants it premultiplied.
            premultiply(&mut color);
        } else {
            // No alpha channel: the AND mask says what's transparent (white).
            let mask = (!info.hbmMask.is_invalid())
                .then(|| read_bitmap(hdc, info.hbmMask, size, size))
                .flatten();
            for (i, p) in color.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let clear = mask.as_ref().is_some_and(|m| m[i * 4] != 0);
                if clear {
                    p.copy_from_slice(&[0, 0, 0, 0]);
                } else {
                    p[3] = 255;
                }
            }
        }
        Some(Pixels { w: size as u32, h: size as u32, data: color })
    })();
    unsafe {
        ReleaseDC(None, hdc);
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        }
    }
    result
}

/// The shell's image for an item: a thumbnail, or its icon.
pub fn shell_image(path: &Path, size: i32) -> Option<Pixels> {
    let w = Wide::os(path.as_os_str());
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(w.pcwstr(), None).ok()?;
        let hbm = factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_RESIZETOFIT).ok()?;
        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(hbm.0),
            size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut c_void),
        );
        let hdc = GetDC(None);
        let data = (got != 0).then(|| read_bitmap(hdc, hbm, bm.bmWidth, bm.bmHeight)).flatten();
        ReleaseDC(None, hdc);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        let mut data = data?;
        if !has_alpha(&data) {
            for p in data.as_chunks_mut::<4>().0 {
                p[3] = 255;
            }
        } else if is_straight_alpha(&data) {
            // Icons come back with straight alpha (thumbnails may not);
            // drawn as premultiplied, their soft edges get light fringes.
            premultiply(&mut data);
        }
        Some(Pixels { w: bm.bmWidth as u32, h: bm.bmHeight as u32, data })
    }
}

/// What resolving an icon source needs from Windows.
struct SystemDeps;

impl IconSourceDeps for SystemDeps {
    fn read_shortcut_link(&self, path: &str) -> Option<ShortcutDetails> {
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            link.cast::<IPersistFile>().ok()?.Load(Wide::new(path).pcwstr(), STGM_READ).ok()?;
            let mut target = [0u16; 1024];
            // Raw (unexpanded) paths; %VAR%s are expanded when resolving.
            let _ = link.GetPath(&mut target, std::ptr::null_mut(), 4 /* SLGP_RAWPATH */);
            let mut icon = [0u16; 1024];
            let mut index = 0;
            let _ = link.GetIconLocation(&mut icon, &mut index);
            let s = |b: &[u16]| Some(from_wide(b)).filter(|s| !s.is_empty());
            Some(ShortcutDetails { target: s(&target), icon: s(&icon), icon_index: index })
        }
    }

    fn read_file(&self, path: &str) -> Option<String> {
        // .url files are usually ANSI or UTF-8; either reads fine for the keys we need.
        std::fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

/// The best image for an item at `size` device pixels.
pub fn item_image(path: &Path, size: i32) -> Option<Pixels> {
    let s = path.to_string_lossy();
    if let Some(src) = resolve_icon_source(&s, &SystemDeps) {
        if let Some(px) = extract_icon(&src.file, src.index, size) {
            return Some(px);
        }
        // The target holds no icon resources (a folder or a document): its
        // own shell image still beats the generic shortcut icon.
        if src.file != s
            && let Some(px) = shell_image(Path::new(&src.file), size)
        {
            return Some(px);
        }
    }
    shell_image(path, size)
}

fn read_reg_string(root: HKEY, key: &str, value: &str) -> Option<String> {
    let (k, v) = (Wide::new(key), Wide::new(value));
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let err = unsafe {
        RegGetValueW(
            root,
            k.pcwstr(),
            v.pcwstr(),
            flags,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut len),
        )
    };
    err.is_ok().then(|| from_wide(&buf))
}

/// The shortcut arrow Explorer draws, honouring a custom `Shell Icons\29`.
fn overlay_source() -> IconSource {
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .find_map(|root| read_reg_string(root, SHELL_ICONS_KEY, SHORTCUT_OVERLAY_VALUE))
        .and_then(|v| parse_icon_location(&v))
        .unwrap_or_else(default_arrow)
}

fn expand(file: &str) -> String {
    sorter::icon_source::expand_env(file, |n| std::env::var(n).ok())
}

/// The shortcut arrow at `size`: a full-size transparent icon with the arrow in
/// its corner. None when the configured arrow is blank (people use that to
/// hide arrows).
pub fn shortcut_overlay(size: i32) -> Option<Pixels> {
    let src = overlay_source();
    let px = extract_icon(&expand(&src.file), src.index, size).or_else(|| {
        let d = default_arrow();
        (d.file != src.file).then(|| extract_icon(&expand(&d.file), d.index, size)).flatten()
    })?;
    has_alpha(&px.data).then_some(px)
}

const MAX_ENTRIES: usize = 300;
/// The pixels of every cached icon together; a large-icon panel at 150% is
/// about 37 KB per icon.
const MAX_BYTES: usize = 6 << 20;

struct Entry {
    key: String,
    mtime: f64,
    bitmap: Option<ID2D1Bitmap>,
    bytes: usize,
    used: u64,
}

/// Recently drawn icons by path and size. A changed file replaces its entry,
/// so stale icons never pile up; the least recently used go first.
#[derive(Default)]
pub struct IconCache {
    entries: Vec<Entry>,
    bytes: usize,
    tick: u64,
    generation: u32,
    overlays: Vec<(i32, Option<ID2D1Bitmap>)>,
}

/// What the cache knows about an item's icon.
pub enum Lookup<'a> {
    /// Loaded; None when the item has no usable image.
    Ready(Option<&'a ID2D1Bitmap>),
    /// Not loaded yet (or the file changed): load it with `load`.
    Missing,
}

fn key(path: &Path, size: i32) -> String {
    format!("{}|{size}", path.to_string_lossy().to_lowercase())
}

impl IconCache {
    /// Drops everything if the render target was recreated (its bitmaps are dead).
    fn check(&mut self, gfx: &Gfx) {
        if self.generation != gfx.generation {
            *self = Self { generation: gfx.generation, ..Self::default() };
        }
    }

    pub fn get(&mut self, gfx: &Gfx, path: &Path, mtime: f64, size: i32) -> Lookup<'_> {
        self.check(gfx);
        let k = key(path, size);
        self.tick += 1;
        let tick = self.tick;
        match self.entries.iter_mut().find(|e| e.key == k) {
            Some(e) if e.mtime == mtime => {
                e.used = tick;
                Lookup::Ready(e.bitmap.as_ref())
            }
            _ => Lookup::Missing,
        }
    }

    pub fn load(&mut self, gfx: &Gfx, path: &Path, mtime: f64, size: i32) {
        self.check(gfx);
        let k = key(path, size);
        if let Some(i) = self.entries.iter().position(|e| e.key == k) {
            let old = self.entries.swap_remove(i);
            self.bytes -= old.bytes;
        }
        let px = item_image(path, size);
        let bytes = px.as_ref().map_or(0, |p| p.data.len());
        let bitmap = px.and_then(|p| gfx.bitmap(p.w, p.h, &p.data).ok());
        self.tick += 1;
        self.entries.push(Entry { key: k, mtime, bitmap, bytes, used: self.tick });
        self.bytes += bytes;
        while self.entries.len() > MAX_ENTRIES || (self.bytes > MAX_BYTES && self.entries.len() > 1)
        {
            let (i, _) =
                self.entries.iter().enumerate().min_by_key(|(_, e)| e.used).expect("not empty");
            let old = self.entries.swap_remove(i);
            self.bytes -= old.bytes;
        }
    }

    pub fn overlay(&mut self, gfx: &Gfx, size: i32) -> Option<&ID2D1Bitmap> {
        self.check(gfx);
        if let Some(i) = self.overlays.iter().position(|(s, _)| *s == size) {
            return self.overlays[i].1.as_ref();
        }
        let bitmap = shortcut_overlay(size).and_then(|p| gfx.bitmap(p.w, p.h, &p.data).ok());
        // A few sizes at most (one per icon size and display scale in use).
        if self.overlays.len() >= 6 {
            self.overlays.remove(0);
        }
        self.overlays.push((size, bitmap));
        self.overlays.last().and_then(|(_, b)| b.as_ref())
    }

    /// Forgets icons, e.g. when every panel is hidden.
    pub fn clear(&mut self) {
        let generation = self.generation;
        *self = Self { generation, ..Self::default() };
    }
}
