//! Monitors and DPI. Config bounds are in DIPs, as Electron saved them; a
//! rectangle converts with the scale of the monitor it's on. Each monitor's
//! DIP rectangle is its physical one divided by its own scale, which keeps
//! the primary monitor (at the origin) identical to Electron's mapping.

use sorter::model::Bounds;
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::core::BOOL;

#[derive(Clone, Copy, Debug)]
pub struct Monitor {
    pub rect: RECT,
    pub work: RECT,
    pub scale: f32,
}

fn dip_of(r: RECT, scale: f32) -> Bounds {
    let d = |v: i32| (v as f32 / scale).round() as i32;
    Bounds::new(d(r.left), d(r.top), d(r.right) - d(r.left), d(r.bottom) - d(r.top))
}

impl Monitor {
    pub fn dip_rect(&self) -> Bounds {
        dip_of(self.rect, self.scale)
    }

    pub fn dip_work(&self) -> Bounds {
        dip_of(self.work, self.scale)
    }

    pub fn dpi(&self) -> u32 {
        (self.scale * 96.0).round() as u32
    }
}

unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let list = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if unsafe { GetMonitorInfoW(m, &mut info) }.as_bool() {
        let (mut x, mut y) = (96, 96);
        let _ = unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y) };
        list.push(Monitor { rect: info.rcMonitor, work: info.rcWork, scale: x as f32 / 96.0 });
    }
    true.into()
}

pub fn monitors() -> Vec<Monitor> {
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        let _ =
            EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut list as *mut _ as isize));
    }
    if list.is_empty() {
        let r = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        list.push(Monitor { rect: r, work: r, scale: 1.0 });
    }
    list
}

fn distance_sq(b: Bounds, (cx, cy): (i32, i32)) -> i64 {
    let dx = (cx - cx.clamp(b.x, b.right())) as i64;
    let dy = (cy - cy.clamp(b.y, b.bottom())) as i64;
    dx * dx + dy * dy
}

/// The monitor a DIP rectangle is on: the one holding its center, else the nearest.
pub fn monitor_for_dip(b: Bounds, mons: &[Monitor]) -> Monitor {
    let c = (b.x + b.width / 2, b.y + b.height / 2);
    *mons.iter().min_by_key(|m| distance_sq(m.dip_rect(), c)).expect("at least one monitor")
}

/// The monitor holding a physical point, else the nearest.
pub fn monitor_for_point(p: POINT, mons: &[Monitor]) -> Monitor {
    let phys = |r: RECT| Bounds::new(r.left, r.top, r.right - r.left, r.bottom - r.top);
    *mons.iter().min_by_key(|m| distance_sq(phys(m.rect), (p.x, p.y))).expect("a monitor")
}

/// A DIP rectangle in physical pixels, with the scale used.
pub fn to_physical(b: Bounds, mons: &[Monitor]) -> (RECT, f32) {
    let s = monitor_for_dip(b, mons).scale;
    let p = |v: i32| (v as f32 * s).round() as i32;
    (RECT { left: p(b.x), top: p(b.y), right: p(b.x + b.width), bottom: p(b.y + b.height) }, s)
}

/// A physical point in DIPs, on the monitor it's on.
pub fn point_to_dip(p: POINT, mons: &[Monitor]) -> (f32, f32) {
    let s = monitor_for_point(p, mons).scale;
    (p.x as f32 / s, p.y as f32 / s)
}
