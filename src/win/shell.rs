//! Shell operations: opening items, the Recycle Bin, known folders, message
//! boxes and the Start with Windows entry.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{HWND, MAX_PATH};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::UI::Shell::{
    FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOFX_RECYCLEONDELETE, FOLDERID_Desktop,
    FileOperation, IFileOperation, ILFree, IShellItem, KF_FLAG_DEFAULT, SEE_MASK_FLAG_NO_UI,
    SHCreateItemFromParsingName, SHELLEXECUTEINFOW, SHGetKnownFolderPath,
    SHOpenFolderAndSelectItems, SHParseDisplayName, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_ICONWARNING, MB_OK, MESSAGEBOX_STYLE, MessageBoxW, SW_SHOWNORMAL,
};
use windows::core::{PCWSTR, w};

use super::{Wide, from_wide};

/// The value name Electron's login item used, so the Rust build replaces it.
pub const RUN_VALUE: &str = "com.shadowhussein.desktopiconssorter";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// Opens an item as a double-click in Explorer would.
pub fn open(path: &Path) -> Result<(), String> {
    let file = Wide::os(path.as_os_str());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI,
        lpFile: file.pcwstr(),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.message())
}

/// Opens Explorer on the item's folder with the item selected.
pub fn show_in_folder(path: &Path) {
    let p = Wide::os(path.as_os_str());
    unsafe {
        let mut pidl = std::ptr::null_mut();
        if SHParseDisplayName(p.pcwstr(), None, &mut pidl, 0, None).is_ok() {
            let _ = SHOpenFolderAndSelectItems(pidl, None, 0);
            ILFree(Some(pidl));
        }
    }
}

/// Moves an item to the Recycle Bin. Nothing is ever deleted outright.
pub fn recycle(path: &Path) -> Result<(), String> {
    let p = Wide::os(path.as_os_str());
    unsafe {
        let op: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(|e| e.message())?;
        op.SetOperationFlags(
            FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOFX_RECYCLEONDELETE,
        )
        .map_err(|e| e.message())?;
        let item: IShellItem =
            SHCreateItemFromParsingName(p.pcwstr(), None).map_err(|e| e.message())?;
        op.DeleteItem(&item, None).map_err(|e| e.message())?;
        op.PerformOperations().map_err(|e| e.message())?;
        if op.GetAnyOperationsAborted().map(|b| b.as_bool()).unwrap_or(false) {
            return Err("The item could not be moved to the Recycle Bin.".into());
        }
    }
    Ok(())
}

/// The user's Desktop folder.
pub fn desktop_dir() -> PathBuf {
    unsafe {
        match SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const c_void));
                PathBuf::from(s)
            }
            Err(_) => std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Desktop"),
        }
    }
}

fn message(owner: Option<HWND>, title: &str, text: &str, style: MESSAGEBOX_STYLE) {
    let (t, x) = (Wide::new(title), Wide::new(text));
    unsafe { MessageBoxW(owner, x.pcwstr(), t.pcwstr(), style | MB_OK) };
}

pub fn error_box(owner: Option<HWND>, title: &str, text: &str) {
    message(owner, title, text, MB_ICONERROR);
}

pub fn warning_box(owner: Option<HWND>, title: &str, text: &str) {
    message(owner, title, text, MB_ICONWARNING);
}

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

/// What the Run entry should hold to start this exe.
fn run_command() -> String {
    format!("\"{}\"", exe_path().display())
}

/// The command in the Start with Windows entry, if there is one.
pub fn run_entry() -> Option<String> {
    let (k, v) = (Wide::new(RUN_KEY), Wide::new(RUN_VALUE));
    let mut buf = [0u16; MAX_PATH as usize * 2];
    let mut len = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            k.pcwstr(),
            v.pcwstr(),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut len),
        )
    };
    err.is_ok().then(|| from_wide(&buf))
}

/// Whether the Start with Windows entry starts this exe.
pub fn starts_with_windows() -> bool {
    run_entry()
        .is_some_and(|c| c.trim_matches('"').eq_ignore_ascii_case(&exe_path().to_string_lossy()))
}

pub fn set_start_with_windows(on: bool) {
    let (k, v) = (Wide::new(RUN_KEY), Wide::new(RUN_VALUE));
    unsafe {
        if on {
            let cmd = Wide::new(&run_command());
            let bytes = (cmd.chars().len() + 1) * 2;
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                k.pcwstr(),
                v.pcwstr(),
                REG_SZ.0,
                Some(cmd.pcwstr().0 as *const c_void),
                bytes as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, k.pcwstr(), v.pcwstr());
        }
    }
}

/// For calls that take an optional window title.
pub fn app_title() -> PCWSTR {
    w!("Desktop Icons Sorter")
}
