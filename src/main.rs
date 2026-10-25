// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod panel;
mod procs;
mod settings;
mod win;

use sorter::config::{ConfigStore, config_path};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, WAIT_OBJECT_0};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Ole::{OleFlushClipboard, OleInitialize, OleUninitialize};
use windows::Win32::System::Threading::{CreateMutexW, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx,
    PM_REMOVE, PeekMessageW, PostMessageW, QS_ALLINPUT, TranslateMessage, WM_QUIT,
};

fn main() {
    // One instance per profile: a second launch shows the first one's panels and exits.
    let suffix = sorter::config::instance_suffix();
    let mutex = win::Wide::new(&format!("Local\\DesktopIconsSorter.Instance{suffix}"));
    let host_title = win::Wide::new(&format!("Desktop Icons Sorter{suffix}"));
    let _instance = unsafe { CreateMutexW(None, true, mutex.pcwstr()) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if let Ok(host) = unsafe { FindWindowW(app::HOST_CLASS, host_title.pcwstr()) } {
            let _ = unsafe { PostMessageW(Some(host), app::WM_APP_SHOW, WPARAM(0), LPARAM(0)) };
        }
        return;
    }

    // OLE (not just COM): drag and drop needs it.
    if unsafe { OleInitialize(None) }.is_err() {
        return;
    }
    procs::register_classes();
    settings::register_class();
    let host = procs::create_host(host_title.pcwstr());
    let path = config_path();
    let first_run = !path.exists();
    let store = ConfigStore::load(path);
    let state = match app::App::new(store, host) {
        Ok(a) => a,
        Err(e) => {
            win::shell::error_box(None, "Desktop Icons Sorter", &format!("Could not start: {e}"));
            return;
        }
    };
    app::install(state);
    app::with_app(|a| a.start(first_run));
    app::run_deferred();

    run_loop();

    app::with_app(|a| a.save_now());
    app::uninstall();
    unsafe {
        // Leaves files copied from a panel on the clipboard after the app quits.
        let _ = OleFlushClipboard();
        OleUninitialize();
    }
}

/// Waits for window messages and for the folder watches' events together.
fn run_loop() {
    let mut msg = MSG::default();
    loop {
        let handles = app::with_app(|a| a.wait_handles()).unwrap_or_default();
        let r = unsafe {
            MsgWaitForMultipleObjectsEx(Some(&handles), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        let i = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        if i < handles.len() {
            app::with_app(|a| a.on_signal(handles[i]));
            app::run_deferred();
            continue;
        }
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                return;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
