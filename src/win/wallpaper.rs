//! The desktop wallpaper: which picture each monitor shows and how Windows
//! places it, so blur and pixelate can show the part behind a panel.

use std::path::Path;

use sorter::background::WallFit;
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    DWPOS_CENTER, DWPOS_FILL, DWPOS_FIT, DWPOS_SPAN, DWPOS_STRETCH, DWPOS_TILE, DesktopWallpaper,
    IDesktopWallpaper,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{PCWSTR, PWSTR};

use super::from_wide;

/// One monitor's wallpaper.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    pub monitor: RECT,
    /// Where the picture is placed: the monitor, or every monitor together when spanned.
    pub area: RECT,
    /// The picture; empty for a plain color.
    pub path: String,
    pub fit: WallFit,
    /// The color around (or instead of) the picture, as 0x00bbggrr.
    pub color: u32,
}

/// Takes a string the shell allocated.
unsafe fn take(s: PWSTR) -> String {
    let text = unsafe { s.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(s.0 as *const _)) };
    text
}

/// The picture `SystemParametersInfo` reports: the one Windows actually
/// shows (a copy it keeps), which is there even when the original isn't.
fn shown_picture() -> String {
    let mut buf = [0u16; 1024];
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr().cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_ok() { from_wide(&buf) } else { String::new() }
}

/// Every monitor's wallpaper; empty if Windows won't say.
pub fn walls() -> Vec<Wall> {
    let Ok(dw) =
        (unsafe { CoCreateInstance::<_, IDesktopWallpaper>(&DesktopWallpaper, None, CLSCTX_ALL) })
    else {
        return Vec::new();
    };
    let fit = match unsafe { dw.GetPosition() }.unwrap_or(DWPOS_FILL) {
        DWPOS_CENTER => WallFit::Center,
        DWPOS_TILE => WallFit::Tile,
        DWPOS_STRETCH => WallFit::Stretch,
        DWPOS_FIT => WallFit::Fit,
        DWPOS_SPAN => WallFit::Span,
        _ => WallFit::Fill,
    };
    let color = unsafe { dw.GetBackgroundColor() }.map_or(0, |c| c.0);
    let fallback = shown_picture();
    let count = unsafe { dw.GetMonitorDevicePathCount() }.unwrap_or(0);
    let mut walls: Vec<Wall> = (0..count)
        .filter_map(|i| unsafe {
            let id = dw.GetMonitorDevicePathAt(i).ok()?;
            let monitor = dw.GetMonitorRECT(PCWSTR(id.0));
            let path = dw.GetWallpaper(PCWSTR(id.0)).map(|p| take(p)).unwrap_or_default();
            take(id);
            let monitor = monitor.ok()?;
            // The original can be gone (moved, or a Spotlight picture); the copy isn't.
            let path = if !path.is_empty() && Path::new(&path).is_file() {
                path
            } else if Path::new(&fallback).is_file() {
                fallback.clone()
            } else {
                String::new()
            };
            Some(Wall { monitor, area: monitor, path, fit, color })
        })
        .collect();
    if fit == WallFit::Span {
        let all = walls.iter().fold(None::<RECT>, |acc, w| {
            let m = w.monitor;
            Some(match acc {
                None => m,
                Some(a) => RECT {
                    left: a.left.min(m.left),
                    top: a.top.min(m.top),
                    right: a.right.max(m.right),
                    bottom: a.bottom.max(m.bottom),
                },
            })
        });
        for w in &mut walls {
            w.area = all.unwrap_or(w.monitor);
        }
    }
    walls
}
