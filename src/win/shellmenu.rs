//! The shell's own context menu and verbs for items, as Explorer shows them:
//! Open with, Send to, Cut, Copy, Delete, Properties and whatever shell
//! extensions add. Also pasting into a folder, and a data object for
//! dragging several items at once.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CMF_CANRENAME, CMF_EXTENDEDVERBS, CMF_NORMAL, CMIC_MASK_PTINVOKE, CMINVOKECOMMANDINFO,
    CMINVOKECOMMANDINFOEX, GCS_VERBW, IContextMenu, IContextMenu2, IContextMenu3, ILFindLastID,
    ILFree, IShellFolder, SHBindToObject, SHBindToParent, SHParseDisplayName,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, MF_SEPARATOR, PostMessageW, SW_SHOWNORMAL,
    SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_DRAWITEM,
    WM_INITMENUPOPUP, WM_MEASUREITEM, WM_MENUCHAR, WM_NULL,
};
use windows::core::{Interface, PCSTR, PCWSTR, PSTR};

use super::menu::{self, Item};
use super::{Wide, from_wide};

/// Shell commands get ids from here up; the app's own items from `EXTRA_BASE`.
const FIRST: u32 = 1;
const LAST: u32 = 0x7fff;
pub const EXTRA_BASE: usize = 0x8000;
/// The Unicode verb fields are filled in (the same bit as SEE_MASK_UNICODE).
const CMIC_MASK_UNICODE: u32 = 0x4000;

thread_local! {
    /// The menu being shown, so its submenus (Send to, Open with) can draw
    /// and fill themselves: they need the owner window's menu messages.
    static ACTIVE: RefCell<Option<(IContextMenu2, Option<IContextMenu3>)>> = const { RefCell::new(None) };
}

/// Absolute item ID lists, freed when dropped.
struct Pidls(Vec<*mut ITEMIDLIST>);

impl Pidls {
    fn parse(paths: &[PathBuf]) -> Self {
        Self(
            paths
                .iter()
                .filter_map(|p| {
                    let w = Wide::os(p.as_os_str());
                    let mut pidl = std::ptr::null_mut();
                    unsafe { SHParseDisplayName(w.pcwstr(), None, &mut pidl, 0, None) }
                        .ok()
                        .map(|_| pidl)
                })
                .collect(),
        )
    }
}

impl Drop for Pidls {
    fn drop(&mut self) {
        for &p in &self.0 {
            unsafe { ILFree(Some(p)) };
        }
    }
}

/// A UI object (context menu, data object) for items that share a folder.
fn ui_object<T: Interface>(owner: HWND, paths: &[PathBuf]) -> Option<T> {
    let pidls = Pidls::parse(paths);
    let first = *pidls.0.first()?;
    unsafe {
        let folder: IShellFolder = SHBindToParent(first, None).ok()?;
        let children: Vec<*const ITEMIDLIST> =
            pidls.0.iter().map(|&p| ILFindLastID(p) as *const ITEMIDLIST).collect();
        folder.GetUIObjectOf(owner, &children, None).ok()
    }
}

/// A data object for dragging or copying several items at once.
pub fn data_object(owner: HWND, paths: &[PathBuf]) -> Option<IDataObject> {
    ui_object(owner, paths)
}

/// The menu a folder's background shows in Explorer (Paste, New, ...).
fn folder_menu(owner: HWND, dir: &Path) -> Option<IContextMenu> {
    let pidls = Pidls::parse(&[dir.to_path_buf()]);
    let pidl = *pidls.0.first()?;
    unsafe {
        let folder: IShellFolder = SHBindToObject(None, pidl, None).ok()?;
        folder.CreateViewObject(owner).ok()
    }
}

enum Verb<'a> {
    /// A command offset into the menu, as QueryContextMenu numbered it.
    Offset(usize),
    Name(&'a str),
}

fn invoke(cm: &IContextMenu, owner: HWND, verb: Verb, at: POINT) -> windows::core::Result<()> {
    let name_a: Vec<u8>;
    let name_w: Wide;
    let (a, w) = match verb {
        // An offset goes in place of the string pointer (MAKEINTRESOURCE).
        Verb::Offset(n) => (PCSTR(n as *const u8), PCWSTR(n as *const u16)),
        Verb::Name(s) => {
            name_a = s.bytes().chain(std::iter::once(0)).collect();
            name_w = Wide::new(s);
            (PCSTR(name_a.as_ptr()), name_w.pcwstr())
        }
    };
    let info = CMINVOKECOMMANDINFOEX {
        cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE,
        hwnd: owner,
        lpVerb: a,
        lpVerbW: w,
        nShow: SW_SHOWNORMAL.0,
        ptInvoke: at,
        ..Default::default()
    };
    unsafe { cm.InvokeCommand(&info as *const _ as *const CMINVOKECOMMANDINFO) }
}

/// The language-independent verb of a menu command ("rename", "delete"...).
fn verb_of(cm: &IContextMenu, offset: usize) -> String {
    let mut buf = [0u16; 64];
    let r = unsafe {
        cm.GetCommandString(
            offset,
            GCS_VERBW,
            None,
            PSTR(buf.as_mut_ptr() as *mut u8),
            buf.len() as u32,
        )
    };
    if r.is_ok() { from_wide(&buf) } else { String::new() }
}

/// What was chosen from an item menu.
pub enum Choice {
    None,
    /// One of the app's own items (its id).
    Extra(usize),
    /// Rename: the app renames in place, as Explorer does.
    Rename,
    /// A shell command, already done.
    Done,
}

/// Shows the shell's context menu for `paths` (which share a folder) at
/// `at`, with the app's `extra` items at the end (ids from `EXTRA_BASE`).
/// `extended` adds the verbs Explorer shows on Shift+right-click.
pub fn show(owner: HWND, paths: &[PathBuf], extra: &[Item], at: POINT, extended: bool) -> Choice {
    let Some(cm) = ui_object::<IContextMenu>(owner, paths) else { return Choice::None };
    unsafe {
        let Ok(hmenu) = CreatePopupMenu() else { return Choice::None };
        let mut flags = CMF_NORMAL | CMF_CANRENAME;
        if extended {
            flags |= CMF_EXTENDEDVERBS;
        }
        let _ = cm.QueryContextMenu(hmenu, 0, FIRST, LAST, flags);
        if !extra.is_empty() {
            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
            menu::append(hmenu, extra);
        }
        let cm2 = cm.cast::<IContextMenu2>().ok();
        let cm3 = cm.cast::<IContextMenu3>().ok();
        ACTIVE.with(|a| *a.borrow_mut() = cm2.map(|c2| (c2, cm3)));
        // Without this the menu doesn't close when clicking elsewhere.
        let _ = SetForegroundWindow(owner);
        let cmd =
            TrackPopupMenu(hmenu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, None, owner, None).0
                as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        ACTIVE.with(|a| *a.borrow_mut() = None);
        let choice = match cmd {
            0 => Choice::None,
            c if c >= EXTRA_BASE => Choice::Extra(c),
            c => {
                let offset = c - FIRST as usize;
                if verb_of(&cm, offset).eq_ignore_ascii_case("rename") {
                    Choice::Rename
                } else {
                    let _ = invoke(&cm, owner, Verb::Offset(offset), at);
                    Choice::Done
                }
            }
        };
        let _ = DestroyMenu(hmenu);
        choice
    }
}

/// Runs a shell verb ("copy", "cut", "properties", ...) on items that share a folder.
pub fn run_verb(owner: HWND, paths: &[PathBuf], verb: &str) -> Result<(), String> {
    let cm = ui_object::<IContextMenu>(owner, paths).ok_or("These items can't be reached.")?;
    run(&cm, owner, verb)
}

/// Pastes what's on the clipboard into `dir`, as Explorer's Paste does
/// (moving after Cut, copying after Copy, asking about name clashes).
pub fn paste_into(owner: HWND, dir: &Path) -> Result<(), String> {
    let cm = folder_menu(owner, dir).ok_or("The folder can't be reached.")?;
    run(&cm, owner, "paste")
}

fn run(cm: &IContextMenu, owner: HWND, verb: &str) -> Result<(), String> {
    unsafe {
        // Some handlers only know their verbs once the menu has been built.
        let hmenu = CreatePopupMenu().map_err(|e| e.message())?;
        let _ = cm.QueryContextMenu(hmenu, 0, FIRST, LAST, CMF_NORMAL);
        let r = invoke(cm, owner, Verb::Name(verb), POINT::default());
        let _ = DestroyMenu(hmenu);
        r.map_err(|e| e.message())
    }
}

/// Whether the clipboard holds files to paste.
pub fn can_paste() -> bool {
    unsafe { IsClipboardFormatAvailable(CF_HDROP.0 as u32) }.is_ok()
}

/// Puts the items' full paths on the clipboard as text, quoted, one per
/// line (Explorer's "Copy as path", Ctrl+Shift+C).
pub fn copy_as_path(owner: HWND, paths: &[PathBuf]) {
    let text: Vec<String> = paths.iter().map(|p| format!("\"{}\"", p.display())).collect();
    let wide: Vec<u16> = text.join("\r\n").encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let dst = GlobalLock(mem) as *mut u16;
            if !dst.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
                let _ = GlobalUnlock(mem);
                // The clipboard owns the memory from here on.
                let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

/// Lets the menu being shown handle the owner window's menu messages, so
/// submenus like Send to and Open with fill in and draw their icons.
pub fn handle_menu_msg(msg: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    if !matches!(msg, WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR) {
        return None;
    }
    ACTIVE.with(|a| {
        let a = a.try_borrow().ok()?;
        let (cm2, cm3) = a.as_ref()?;
        let mut result = LRESULT(0);
        let ok = match cm3 {
            Some(c3) => unsafe { c3.HandleMenuMsg2(msg, w, l, Some(&mut result)) }.is_ok(),
            None => unsafe { cm2.HandleMenuMsg(msg, w, l) }.is_ok(),
        };
        ok.then_some(result)
    })
}
