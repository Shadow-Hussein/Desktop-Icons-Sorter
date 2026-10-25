// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The Win32 app (panels, tray, message loop) lands in milestone 1.
    let path = sorter::config::config_path();
    let config = sorter::config::ConfigStore::load(path).config().clone();
    println!("{} panel(s) in config", config.panels.len());
}
