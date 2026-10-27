//! Pure logic of Desktop Icons Sorter. Nothing here touches Win32, so it all
//! runs (and is tested) with plain `cargo test`.

pub mod background;
pub mod catcher;
pub mod config;
pub mod folder;
pub mod geometry;
pub mod grid;
pub mod icon_source;
pub mod model;
pub mod overlay;
pub mod panel_layout;
pub mod stack;
pub mod tray_art;
