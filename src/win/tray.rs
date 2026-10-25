//! The notification area icon.

use sorter::tray_art::tray_icon_pixels;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, HGDIOBJ};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON,
};

use super::utf16;

pub struct Tray {
    hwnd: HWND,
    message: u32,
    icon: HICON,
}

/// The tray icon drawn at the small-icon size for this display.
fn make_icon() -> HICON {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let px = tray_icon_pixels(size as usize);
    unsafe {
        let color = CreateBitmap(size, size, 1, 32, Some(px.as_ptr() as *const _));
        // With a 32-bit color bitmap, the alpha channel decides transparency; the mask is unused.
        let mask_bits = vec![0u8; (size as usize).div_ceil(16) * 2 * size as usize];
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bits.as_ptr() as *const _));
        let info =
            ICONINFO { fIcon: true.into(), hbmColor: color, hbmMask: mask, ..Default::default() };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}

impl Tray {
    /// Adds the icon; clicks arrive at `hwnd` as `message`.
    pub fn new(hwnd: HWND, message: u32) -> Self {
        let tray = Self { hwnd, message, icon: make_icon() };
        tray.add();
        tray
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut d = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: self.message,
            hIcon: self.icon,
            ..Default::default()
        };
        let tip = utf16("Desktop Icons Sorter");
        d.szTip[..tip.len()].copy_from_slice(&tip);
        d
    }

    /// Adds the icon again, e.g. after Explorer restarted and dropped it.
    pub fn add(&self) {
        let _ = unsafe { Shell_NotifyIconW(NIM_ADD, &self.data()) };
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data());
            let _ = DestroyIcon(self.icon);
        }
    }
}
