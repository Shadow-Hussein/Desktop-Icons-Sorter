//! Dark popup menus. Win32 menus (the app's and the shell's context menu)
//! are drawn light unless the process opts in to the dark theme through
//! uxtheme's undocumented ordinals, as Explorer and Notepad++ do. With
//! "allow dark", menus follow the Windows app theme setting.

use std::sync::OnceLock;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{PCSTR, w};

/// uxtheme.dll ordinals, stable since Windows 10 1903.
const SET_PREFERRED_APP_MODE: usize = 135;
const FLUSH_MENU_THEMES: usize = 136;
const REFRESH_IMMERSIVE_COLOR_POLICY_STATE: usize = 104;
/// PreferredAppMode::AllowDark (also `true` for 1809's AllowDarkModeForApp).
const ALLOW_DARK: i32 = 1;

struct UxTheme(HMODULE);

// SAFETY: a module handle is just an address; uxtheme stays loaded for the
// life of the process.
unsafe impl Send for UxTheme {}
unsafe impl Sync for UxTheme {}

fn uxtheme() -> Option<HMODULE> {
    static LIB: OnceLock<Option<UxTheme>> = OnceLock::new();
    LIB.get_or_init(|| {
        unsafe { LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .ok()
            .map(UxTheme)
    })
    .as_ref()
    .map(|l| l.0)
}

/// Calls an ordinal export taking one i32 (or nothing, ignored) and
/// returning a value that doesn't matter here.
fn call(ordinal: usize, arg: Option<i32>) {
    let Some(lib) = uxtheme() else { return };
    // An ordinal goes in place of the name (MAKEINTRESOURCEA).
    let Some(f) = (unsafe { GetProcAddress(lib, PCSTR(ordinal as *const u8)) }) else { return };
    unsafe {
        match arg {
            Some(a) => {
                let f: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
                f(a);
            }
            None => {
                let f: unsafe extern "system" fn() = std::mem::transmute(f);
                f();
            }
        }
    }
}

/// Lets menus follow the Windows dark or light app theme. Call once at start.
pub fn allow_dark_menus() {
    call(SET_PREFERRED_APP_MODE, Some(ALLOW_DARK));
    refresh();
}

/// The Windows theme changed: menus pick it up from the next one shown.
pub fn refresh() {
    call(REFRESH_IMMERSIVE_COLOR_POLICY_STATE, None);
    call(FLUSH_MENU_THEMES, None);
}
