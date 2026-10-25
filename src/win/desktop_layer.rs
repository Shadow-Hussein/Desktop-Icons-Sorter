//! Keeps panel windows on the desktop layer.
//!
//! Each panel is made an *owned* window of Progman (the desktop shell window)
//! and kept at the bottom of the z-order. Owned windows always stay above
//! their owner, so panels sit directly on the desktop, below normal app
//! windows, and they stay visible on "Show desktop" (Win+D). Unlike parenting
//! into the wallpaper WorkerW, panels keep getting mouse input.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GWLP_HWNDPARENT, GetWindowLongPtrW, HWND_BOTTOM, IsWindow, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos,
};
use windows::core::{PCWSTR, w};

fn progman() -> Option<HWND> {
    unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.ok().filter(|h| !h.is_invalid())
}

/// Makes `hwnd` a desktop-level window. Safe to call repeatedly.
///
/// The owner is set after creation, as the Electron app did: Explorer
/// restarting destroys Progman, and windows owned this way survive that
/// (they're re-owned when the new Progman appears, see `needs_reattach`).
pub fn attach(hwnd: HWND) {
    let Some(owner) = progman() else { return };
    unsafe {
        if GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) != owner.0 as isize {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner.0 as isize);
        }
    }
    send_to_bottom(hwnd);
}

/// Pushes `hwnd` to the bottom of the z-order (just above the desktop).
pub fn send_to_bottom(hwnd: HWND) {
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    let _ = unsafe { SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, flags) };
}

/// True when the window's owner is gone or isn't the current Progman, as
/// after Explorer restarted.
pub fn needs_reattach(hwnd: HWND) -> bool {
    let owner = HWND(unsafe { GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) } as *mut _);
    owner.is_invalid()
        || !unsafe { IsWindow(Some(owner)) }.as_bool()
        || progman().is_none_or(|p| p != owner)
}
