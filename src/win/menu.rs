//! Popup menus. They run a modal loop, so they're shown outside any borrow
//! of the app (see `app::Deferred`) and return the chosen command.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, HMENU, MF_CHECKED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, PostMessageW, SetForegroundWindow, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
};

use super::Wide;

pub enum Item {
    Cmd(usize, String),
    /// A command with a check mark when true.
    Check(usize, String, bool),
    Sep,
    Sub(String, Vec<Item>),
}

impl Item {
    pub fn cmd(id: usize, label: &str) -> Self {
        Self::Cmd(id, label.into())
    }

    pub fn check(id: usize, label: &str, on: bool) -> Self {
        Self::Check(id, label.into(), on)
    }
}

fn build(items: &[Item]) -> HMENU {
    let menu = unsafe { CreatePopupMenu() }.unwrap_or_default();
    append(menu, items);
    menu
}

/// Adds `items` to the end of an existing menu.
pub fn append(menu: HMENU, items: &[Item]) {
    unsafe {
        for item in items {
            let _ = match item {
                Item::Cmd(id, label) => {
                    AppendMenuW(menu, MF_STRING, *id, Wide::new(label).pcwstr())
                }
                Item::Check(id, label, on) => {
                    let flags = MF_STRING | if *on { MF_CHECKED } else { MF_UNCHECKED };
                    AppendMenuW(menu, flags, *id, Wide::new(label).pcwstr())
                }
                Item::Sep => AppendMenuW(menu, MF_SEPARATOR, 0, None),
                Item::Sub(label, children) => {
                    let sub = build(children);
                    AppendMenuW(menu, MF_POPUP, sub.0 as usize, Wide::new(label).pcwstr())
                }
            };
        }
    }
}

/// Shows the menu at `at` (screen pixels) or the cursor; returns the chosen
/// id, or 0 when dismissed.
pub fn show(items: &[Item], owner: HWND, at: Option<POINT>) -> usize {
    let menu = build(items);
    let mut pt = POINT::default();
    unsafe {
        match at {
            Some(p) => pt = p,
            None => {
                let _ = GetCursorPos(&mut pt);
            }
        }
        // Without this the menu doesn't close when clicking elsewhere.
        let _ = SetForegroundWindow(owner);
        let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY;
        let cmd = TrackPopupMenu(menu, flags, pt.x, pt.y, None, owner, None);
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        // Destroys the submenus too.
        let _ = DestroyMenu(menu);
        cmd.0 as usize
    }
}
