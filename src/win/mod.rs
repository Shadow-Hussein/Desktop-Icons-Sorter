//! Win32 side of the app: windows, drawing, shell calls. Everything here runs
//! on the one UI thread.

pub mod desktop_layer;
pub mod dialogs;
pub mod dpi;
pub mod dragdrop;
pub mod edit;
pub mod icons;
pub mod menu;
pub mod render;
pub mod shell;
pub mod shellmenu;
pub mod tray;
pub mod watch;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::core::PCWSTR;

/// A NUL-terminated UTF-16 string that stays alive while its PCWSTR is used.
pub struct Wide(Vec<u16>);

impl Wide {
    pub fn new(s: &str) -> Self {
        Self(s.encode_utf16().chain(std::iter::once(0)).collect())
    }

    pub fn os(s: &OsStr) -> Self {
        Self(s.encode_wide().chain(std::iter::once(0)).collect())
    }

    pub fn pcwstr(&self) -> PCWSTR {
        PCWSTR::from_raw(self.0.as_ptr())
    }

    /// Without the terminating NUL, for APIs that take a length.
    pub fn chars(&self) -> &[u16] {
        &self.0[..self.0.len() - 1]
    }
}

/// UTF-16 without a terminator, for DirectWrite.
pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// A string from a NUL-terminated (or full) UTF-16 buffer.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub fn loword(v: usize) -> u16 {
    (v & 0xffff) as u16
}

pub fn hiword(v: usize) -> u16 {
    ((v >> 16) & 0xffff) as u16
}

/// Signed client coordinates from a mouse message's LPARAM.
pub fn point_of(l: LPARAM) -> (i32, i32) {
    let v = l.0 as usize;
    (loword(v) as i16 as i32, hiword(v) as i16 as i32)
}

pub fn wheel_delta(w: WPARAM) -> i16 {
    hiword(w.0) as i16
}

/// Milliseconds since the system started, for timing that doesn't jump with the clock.
pub fn now_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}
