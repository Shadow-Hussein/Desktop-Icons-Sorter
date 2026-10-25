//! Modal dialogs: questions with custom buttons, picking a folder, picking a color.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TD_INFORMATION_ICON, TDF_ALLOW_DIALOG_CANCELLATION,
    TDF_POSITION_RELATIVE_TO_WINDOW, TDF_USE_COMMAND_LINKS, TaskDialogIndirect,
};
use windows::Win32::UI::Shell::{
    FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
    SIGDN_FILESYSPATH,
};
use windows::core::PCWSTR;

use super::Wide;

/// Asks a question with one button per answer; returns the chosen button's
/// index, or None when cancelled. With `links`, buttons are big command links.
pub fn ask(
    owner: Option<HWND>,
    title: &str,
    main: &str,
    detail: &str,
    buttons: &[&str],
    links: bool,
) -> Option<usize> {
    let (t, m, d) = (Wide::new(title), Wide::new(main), Wide::new(detail));
    let labels: Vec<Wide> = buttons.iter().map(|b| Wide::new(b)).collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| TASKDIALOG_BUTTON { nButtonID: 100 + i as i32, pszButtonText: l.pcwstr() })
        .collect();
    let mut flags = TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW;
    if links {
        flags |= TDF_USE_COMMAND_LINKS;
    }
    let mut config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: owner.unwrap_or_default(),
        dwFlags: flags,
        pszWindowTitle: t.pcwstr(),
        pszMainInstruction: m.pcwstr(),
        pszContent: if detail.is_empty() { PCWSTR::null() } else { d.pcwstr() },
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: 100,
        ..Default::default()
    };
    config.Anonymous1.pszMainIcon = TD_INFORMATION_ICON;
    let mut pressed = 0;
    unsafe { TaskDialogIndirect(&config, Some(&mut pressed), None, None) }.ok()?;
    (pressed >= 100).then(|| (pressed - 100) as usize)
}

/// Lets the user pick a folder, starting in `start`.
pub fn choose_folder(owner: Option<HWND>, title: &str, start: &Path) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_PICKFOLDERS).ok()?;
        let t = Wide::new(title);
        let _ = dialog.SetTitle(t.pcwstr());
        let s = Wide::os(start.as_os_str());
        if let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(s.pcwstr(), None) {
            let _ = dialog.SetFolder(&folder);
        }
        dialog.Show(owner).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as *const _));
        path.map(PathBuf::from)
    }
}

/// The custom colors the color dialog remembers while the app runs.
static mut CUSTOM_COLORS: [COLORREF; 16] = [COLORREF(0x00ffffff); 16];

/// Lets the user pick a color, starting from `hex` (#rrggbb); returns #rrggbb.
pub fn choose_color(owner: Option<HWND>, hex: &str) -> Option<String> {
    let c = |i: usize| {
        hex.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(0) as u32
    };
    let initial = COLORREF(c(1) | (c(3) << 8) | (c(5) << 16));
    let mut cc = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner.unwrap_or_default(),
        rgbResult: initial,
        // Only the UI thread shows color dialogs.
        lpCustColors: std::ptr::addr_of_mut!(CUSTOM_COLORS) as *mut COLORREF,
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    if !unsafe { ChooseColorW(&mut cc) }.as_bool() {
        return None;
    }
    let v = cc.rgbResult.0;
    Some(format!("#{:02x}{:02x}{:02x}", v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff))
}
