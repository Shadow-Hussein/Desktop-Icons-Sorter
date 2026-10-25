//! A one-line text box for renaming in place. Panels are layered windows,
//! which can't show child controls, so the box is a small popup of its own,
//! owned by the panel so it stays above it.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject,
    FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS,
};
use windows::Win32::UI::Controls::EM_SETSEL;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, ES_AUTOHSCROLL, ES_CENTER, GetWindowTextLengthW,
    GetWindowTextW, PostMessageW, SW_SHOW, SendMessageW, ShowWindow, WINDOW_STYLE, WM_CHAR,
    WM_KEYDOWN, WM_KILLFOCUS, WM_SETFONT, WS_BORDER, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::w;

use super::Wide;

pub struct EditBox {
    pub hwnd: HWND,
    font: HFONT,
}

/// Where the box reports that editing ended: `msg` posted to `to`, with
/// WPARAM 1 to keep the text, 0 to cancel.
#[derive(Clone, Copy)]
pub struct Done {
    pub to: HWND,
    pub msg: u32,
}

const SUBCLASS_ID: usize = 1;

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let done = unsafe { *(data as *const Done) };
    let finish = |keep: bool| unsafe {
        let _ = PostMessageW(Some(done.to), done.msg, WPARAM(keep as usize), LPARAM(0));
    };
    match msg {
        WM_KEYDOWN if w.0 == VK_RETURN.0 as usize => {
            finish(true);
            return LRESULT(0);
        }
        WM_KEYDOWN if w.0 == VK_ESCAPE.0 as usize => {
            finish(false);
            return LRESULT(0);
        }
        // Swallow the characters, or the box beeps.
        WM_CHAR if w.0 == 0x0d || w.0 == 0x1b => return LRESULT(0),
        WM_KILLFOCUS => finish(true),
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, w, l) }
}

impl EditBox {
    /// Shows a box over `rect` (screen pixels) holding `text`, all selected.
    pub fn new(
        owner: HWND,
        rect: RECT,
        text: &str,
        font_px: i32,
        bold: bool,
        done: Done,
    ) -> Option<Self> {
        unsafe {
            let font = CreateFontW(
                -font_px,
                0,
                0,
                0,
                if bold { FW_SEMIBOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                FF_DONTCARE.0 as u32,
                w!("Segoe UI"),
            );
            let t = Wide::new(text);
            let style = WS_POPUP | WS_BORDER | WINDOW_STYLE((ES_AUTOHSCROLL | ES_CENTER) as u32);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("EDIT"),
                t.pcwstr(),
                style,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                Some(owner),
                None,
                None,
                None,
            )
            .ok()?;
            SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            // Leaked into the subclass's ref data; freed when the box is dropped.
            let data = Box::into_raw(Box::new(done)) as usize;
            let _ = SetWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID, data);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetFocus(Some(hwnd));
            SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            Some(Self { hwnd, font })
        }
    }

    pub fn text(&self) -> String {
        unsafe {
            let len = GetWindowTextLengthW(self.hwnd) as usize;
            let mut buf = vec![0u16; len + 1];
            let n = GetWindowTextW(self.hwnd, &mut buf) as usize;
            String::from_utf16_lossy(&buf[..n])
        }
    }
}

impl Drop for EditBox {
    fn drop(&mut self) {
        unsafe {
            let mut data = 0usize;
            let _ = windows::Win32::UI::Shell::GetWindowSubclass(
                self.hwnd,
                Some(edit_proc),
                SUBCLASS_ID,
                Some(&mut data),
            );
            let _ = RemoveWindowSubclass(self.hwnd, Some(edit_proc), SUBCLASS_ID);
            let _ = DestroyWindow(self.hwnd);
            if data != 0 {
                drop(Box::from_raw(data as *mut Done));
            }
            let _ = DeleteObject(HGDIOBJ(self.font.0));
        }
    }
}
