//! The app: config, every panel window, the tray and what they do. One
//! instance lives in a thread-local on the UI thread; window procedures (see
//! `procs.rs`) reach it through `with_app`.
//!
//! Modal UI (menus, dialogs, drag and drop) runs its own message loop, which
//! calls back into the window procedures. So it never runs while the app is
//! borrowed: handlers queue it as a `Deferred` and it runs once the handler
//! has returned.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use sorter::background::{blur_radius, blur_reduce, pixel_block};
use sorter::catcher::{self, Catcher};
use sorter::config::{
    ConfigStore, SAVE_DEBOUNCE_MS, create_panel, duplicate_panel, folder_name_from_title,
    with_look_of,
};
use sorter::folder::{
    self, copy_into, list_items, move_into, place_in_order, prune_cells, rename_in_cells,
    rename_in_order, rename_item, same_path, sync_order,
};
use sorter::geometry::clamp_to_work_areas;
use sorter::grid::{layout_grid, place_at};
use sorter::model::{AddMode, BgEffect, Bounds, MAX_PANEL_ITEMS, PanelConfig, PlacedItem, SortBy};
use sorter::panel_layout::Rect;
use sorter::stack::stack_move;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, w};

use crate::panel::{self, Anim, DropState, Panel, Scene, cells_of, current_layout, window_bounds};
use crate::procs;
use crate::settings::{Layer, SettingsWin};
use crate::win::bgimage::{BgImages, Image, Kind, Want, wall_slot};
use crate::win::dpi::{self, Monitor};
use crate::win::edit::{Done, EditBox};
use crate::win::icons::{IconCache, Lookup};
use crate::win::render::{Font, Gfx};
use crate::win::tray::Tray;
use crate::win::wallpaper::{self, Wall};
use crate::win::watch::{DirWatch, Report};
use crate::win::{Wide, desktop_layer, dialogs, dragdrop, now_ms, shell, shellmenu};

pub const WM_APP_TRAY: u32 = WM_APP + 1;
/// Sent by a second instance: show the panels.
pub const WM_APP_SHOW: u32 = WM_APP + 2;
/// Load icons a panel drew placeholders for.
pub const WM_APP_ICONS: u32 = WM_APP + 3;
/// A rename box finished (WPARAM 1 keeps the text).
pub const WM_APP_EDIT_DONE: u32 = WM_APP + 4;

pub const HOST_CLASS: PCWSTR = w!("DesktopIconsSorter.Host");
pub const PANEL_CLASS: PCWSTR = w!("DesktopIconsSorter.Panel");

/// Host window timers.
pub const TIMER_SAVE: usize = 1;
const TIMER_CATCH: usize = 2;
/// Panel window timers.
pub const TIMER_REFRESH: usize = 1;
pub const TIMER_ANIM: usize = 2;
/// How long a burst of folder changes is collected before listing it again.
const REFRESH_MS: u32 = 150;
/// How long loading icons may hold up the UI before it redraws and goes on.
const ICON_BUDGET_MS: u64 = 40;
/// One animation frame.
pub const FRAME_MS: u32 = 15;

/// Modal work queued by a handler (see the module docs).
pub enum Deferred {
    Error {
        title: String,
        text: String,
    },
    Open(PathBuf),
    TrayMenu,
    /// A panel menu at `at` (screen pixels), or at the cursor.
    Menu {
        panel: String,
        kind: MenuKind,
        at: Option<POINT>,
    },
    DragOut {
        panel: String,
        paths: Vec<PathBuf>,
    },
    Recycle(Vec<PathBuf>),
    /// A shell verb ("copy", "cut", "properties") on a panel's items.
    Verb {
        panel: String,
        paths: Vec<PathBuf>,
        verb: &'static str,
    },
    /// Paste the clipboard's files into a panel.
    Paste(String),
    DeletePanel(String),
    ChooseColor {
        owner: HWND,
        panel: String,
        layer: Layer,
    },
    ChooseFolder {
        owner: HWND,
        panel: String,
    },
    ChooseImage {
        owner: HWND,
        panel: String,
    },
    LookMenu {
        owner: HWND,
        panel: String,
    },
    SortMenu {
        owner: HWND,
        panel: String,
    },
    Quit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuKind {
    Title,
    Background,
    /// The shell's menu for these items (the selection, or the one clicked).
    Items(Vec<PathBuf>),
}

/// What a rename box is renaming.
#[derive(Clone, Debug, PartialEq)]
pub enum EditTarget {
    Title(String),
    Item(PathBuf),
}

pub struct App {
    pub store: ConfigStore,
    pub gfx: Gfx,
    pub icons: IconCache,
    pub bg_images: BgImages,
    /// Each monitor's wallpaper, read when a panel first needs it (blur, pixelate).
    walls: Option<Vec<Wall>>,
    pub panels: Vec<Panel>,
    pub host: HWND,
    tray: Option<Tray>,
    pub hidden: bool,
    pub desktop_dir: PathBuf,
    pub panels_root: PathBuf,
    pub mons: Vec<Monitor>,
    deferred: VecDeque<Deferred>,
    pub taskbar_created: u32,
    /// Items being dragged out of a panel: (panel id, paths).
    pub dragging: Option<(String, Vec<PathBuf>)>,
    edit: Option<(EditBox, EditTarget)>,
    pub settings: Vec<SettingsWin>,
    /// Watches the desktop while a panel catches new items.
    catcher: Option<(Catcher, DirWatch)>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static RUNNING_DEFERRED: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` on the app; None when there's no app yet or it's busy (a message
/// sent while a handler runs, e.g. by SetWindowPos, gets default handling).
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn install(app: App) {
    APP.with(|a| *a.borrow_mut() = Some(app));
}

pub fn uninstall() {
    let app = APP.with(|a| a.borrow_mut().take());
    drop(app);
}

/// Runs queued modal work, outside any borrow of the app.
pub fn run_deferred() {
    if RUNNING_DEFERRED.with(|r| r.replace(true)) {
        return; // An outer call is already working through the queue.
    }
    while let Some(Some(d)) = with_app(|a| a.deferred.pop_front()) {
        execute(d);
    }
    RUNNING_DEFERRED.with(|r| r.set(false));
}

fn execute(d: Deferred) {
    match d {
        Deferred::Error { title, text } => shell::error_box(None, &title, &text),
        Deferred::Open(path) => {
            if let Err(e) = shell::open(&path) {
                shell::error_box(None, "Could not open item", &e);
            }
        }
        Deferred::TrayMenu => procs::tray_menu(),
        Deferred::Menu { panel, kind, at } => procs::panel_menu(&panel, kind, at),
        Deferred::DragOut { panel, paths } => {
            let Some(hwnd) =
                with_app(|a| a.panel_by_id(&panel).map(|i| a.panels[i].hwnd)).flatten()
            else {
                return;
            };
            dragdrop::drag_out(hwnd, &paths);
            with_app(|a| {
                a.dragging = None;
                if let Some(i) = a.panel_by_id(&panel) {
                    a.panels[i].drop = None;
                    a.paint(i);
                }
            });
        }
        Deferred::Recycle(paths) => {
            let errors: Vec<String> = paths
                .iter()
                .filter_map(|p| {
                    shell::recycle(p).err().map(|e| {
                        format!(
                            "{}
  {e}",
                            p.display()
                        )
                    })
                })
                .collect();
            if !errors.is_empty() {
                shell::error_box(
                    None,
                    "Could not delete item",
                    &errors.join(
                        "

",
                    ),
                );
            }
        }
        Deferred::Verb { panel, paths, verb } => {
            let Some(hwnd) =
                with_app(|a| a.panel_by_id(&panel).map(|i| a.panels[i].hwnd)).flatten()
            else {
                return;
            };
            if let Err(e) = shellmenu::run_verb(hwnd, &paths, verb) {
                shell::error_box(Some(hwnd), "Could not do that", &e);
            }
        }
        Deferred::Paste(panel) => {
            let Some((hwnd, dir)) = with_app(|a| {
                let i = a.panel_by_id(&panel)?;
                Some((a.panels[i].hwnd, PathBuf::from(&a.config_of(i)?.folder_path)))
            })
            .flatten() else {
                return;
            };
            if let Err(e) = shellmenu::paste_into(hwnd, &dir) {
                shell::error_box(Some(hwnd), "Could not paste", &e);
            }
        }
        Deferred::DeletePanel(id) => delete_panel_flow(&id),
        Deferred::ChooseColor { owner, panel, layer } => choose_color_flow(owner, &panel, layer),
        Deferred::ChooseFolder { owner, panel } => choose_folder_flow(owner, &panel),
        Deferred::ChooseImage { owner, panel } => choose_image_flow(owner, &panel),
        Deferred::SortMenu { owner, panel } => crate::settings::sort_menu(owner, &panel),
        Deferred::LookMenu { owner, panel } => crate::settings::look_menu(owner, &panel),
        Deferred::Quit => unsafe { PostQuitMessage(0) },
    }
}

pub fn hinstance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe { GetModuleHandleW(None) }.map(Into::into).unwrap_or_default()
}

/// Whether the user wants animations (Settings > Accessibility > Visual effects).
fn animations_on() -> bool {
    let mut on = BOOL(1);
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    on.as_bool()
}

impl App {
    pub fn new(store: ConfigStore, host: HWND) -> windows::core::Result<Self> {
        Ok(Self {
            store,
            gfx: Gfx::new()?,
            icons: IconCache::default(),
            bg_images: BgImages::default(),
            walls: None,
            panels: Vec::new(),
            host,
            tray: None,
            hidden: false,
            // Tests point this at a throwaway folder instead of the real Desktop.
            desktop_dir: std::env::var_os("DESKTOP_SORTER_DESKTOP")
                .map(PathBuf::from)
                .unwrap_or_else(shell::desktop_dir),
            panels_root: sorter::config::panels_root_dir(),
            mons: dpi::monitors(),
            deferred: VecDeque::new(),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            dragging: None,
            edit: None,
            settings: Vec::new(),
            catcher: None,
        })
    }

    /// `first_run`: there was no config file yet.
    pub fn start(&mut self, first_run: bool) {
        self.tray = Some(Tray::new(self.host, WM_APP_TRAY));
        self.sync_start_with_windows(first_run);
        if self.store.config().panels.is_empty() {
            self.add_panel("Apps");
        }
        let ids: Vec<String> = self.store.config().panels.iter().map(|p| p.id.clone()).collect();
        for id in ids {
            self.open_panel(&id);
        }
        self.sync_catcher();
    }

    /// Keeps the Start with Windows entry in step with the saved setting. The
    /// installer adds the entry, so on a first run its presence says this
    /// copy was installed (the portable exe isn't, and doesn't start with
    /// Windows). Dev builds and throwaway profiles leave the registry alone.
    fn sync_start_with_windows(&mut self, first_run: bool) {
        if cfg!(debug_assertions) || std::env::var_os("DESKTOP_SORTER_USER_DATA").is_some() {
            return;
        }
        if first_run && shell::starts_with_windows() {
            self.store.update(|c| c.start_with_windows = true);
            self.changed();
        }
        if self.store.config().start_with_windows {
            // Also points an entry the Electron app made at this exe.
            shell::set_start_with_windows(true);
        } else if shell::starts_with_windows() {
            shell::set_start_with_windows(false);
        }
    }

    pub fn defer(&mut self, d: Deferred) {
        self.deferred.push_back(d);
    }

    pub fn error(&mut self, title: &str, text: &str) {
        self.defer(Deferred::Error { title: title.into(), text: text.into() });
    }

    /// Saves the config a moment after the last change, and shows the change
    /// in any open settings window.
    pub fn changed(&mut self) {
        if self.store.is_dirty() {
            unsafe { SetTimer(Some(self.host), TIMER_SAVE, SAVE_DEBOUNCE_MS, None) };
        }
        self.refresh_settings();
    }

    /// Changes app-wide settings; panels show some of them (lock, stacking).
    pub fn update_app(&mut self, f: impl FnOnce(&mut sorter::model::AppConfig)) {
        self.store.update(f);
        self.changed();
        self.paint_all();
    }

    pub fn save_now(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.host), TIMER_SAVE);
        }
        if let Err(e) = self.store.flush() {
            eprintln!("[store] save failed: {e}");
        }
    }

    pub fn panel_index(&self, hwnd: HWND) -> Option<usize> {
        self.panels.iter().position(|p| p.hwnd == hwnd)
    }

    pub fn panel_by_id(&self, id: &str) -> Option<usize> {
        self.panels.iter().position(|p| p.id == id)
    }

    pub fn config_of(&self, i: usize) -> Option<&PanelConfig> {
        self.store.panel(&self.panels[i].id)
    }

    // ------------------------------------------------------------ lifecycle

    fn work_areas(&self) -> Vec<Bounds> {
        self.mons.iter().map(Monitor::dip_work).collect()
    }

    fn unique_folder(&self, title: &str) -> PathBuf {
        let base = folder_name_from_title(title);
        let used = |dir: &Path| {
            dir.exists()
                || self
                    .store
                    .config()
                    .panels
                    .iter()
                    .any(|p| same_path(Path::new(&p.folder_path), dir))
        };
        let mut dir = self.panels_root.join(&base);
        let mut n = 2;
        while used(&dir) {
            dir = self.panels_root.join(format!("{base} ({n})"));
            n += 1;
        }
        dir
    }

    /// Adds a panel to the config (not yet on screen); returns its id.
    fn add_panel(&mut self, title: &str) -> String {
        let area = self.mons.first().map(Monitor::dip_work).unwrap_or_default();
        let n = self.store.config().panels.len() as i32;
        let offset = (n % 10) * 32;
        let bounds = Bounds::new(area.x + 40 + offset, area.y + 40 + offset, 360, 240);
        let folder = self.unique_folder(title);
        let panel = create_panel(title, bounds, &folder.to_string_lossy());
        let id = panel.id.clone();
        self.store.update(|c| c.panels.push(panel));
        self.changed();
        id
    }

    pub fn new_panel(&mut self) {
        let n = self.store.config().panels.len() + 1;
        let title = if n == 1 { "Apps".to_string() } else { format!("Panel {n}") };
        let id = self.add_panel(&title);
        self.open_panel(&id);
    }

    /// The monitor wallpapers panel `p` needs, when its effect shows the
    /// wallpaper: those of the monitors its window at `rect` overlaps. Reads
    /// the wallpaper settings the first time.
    fn wall_wants(&mut self, p: &PanelConfig, rect: RECT, scale: f32) -> Vec<Want> {
        if !p.bg_effect.uses_wallpaper() {
            return Vec::new();
        }
        let walls = self.walls.get_or_insert_with(wallpaper::walls);
        walls
            .iter()
            .filter(|w| {
                let m = w.monitor;
                m.left < rect.right
                    && rect.left < m.right
                    && m.top < rect.bottom
                    && rect.top < m.bottom
            })
            .map(|w| effect_want(p, Want::wallpaper(w, scale), scale))
            .collect()
    }

    /// The monitor wallpapers behind panel `p`'s window at `rect`, each with
    /// where it goes from the window's corner (DIPs at `scale`), and whether
    /// they're all decoded as wanted. Placed from where the window is right
    /// now, so they stay put on screen while it moves or collapses.
    pub fn wall_images(
        &mut self,
        p: &PanelConfig,
        rect: RECT,
        scale: f32,
    ) -> (Vec<(Image, Rect)>, bool) {
        let mut fresh = true;
        let mut out = Vec::new();
        for want in self.wall_wants(p, rect, scale) {
            let (image, ok) = self.bg_images.get(&self.gfx, &wall_slot(&want), &want);
            fresh &= ok;
            if let (Some(image), Kind::Wallpaper { rect: m, .. }) = (image, &want.kind) {
                let at = Rect::new(
                    (m.0 - rect.left) as f32 / scale,
                    (m.1 - rect.top) as f32 / scale,
                    m.2 as f32 / scale,
                    m.3 as f32 / scale,
                );
                out.push((image, at));
            }
        }
        (out, fresh)
    }

    /// Decodes the monitor wallpapers panel `i` is missing, and drops those no panel uses.
    fn load_walls(&mut self, i: usize) {
        let Some(cfg) = self.config_of(i).cloned() else { return };
        let (rect, scale) = (self.panels[i].rect, self.panels[i].scale);
        for want in self.wall_wants(&cfg, rect, scale) {
            let slot = wall_slot(&want);
            if !self.bg_images.get(&self.gfx, &slot, &want).1 {
                // Paths are lower-cased in `Want`, which opens the same file on Windows.
                let path = want.path.clone();
                self.bg_images.load(&self.gfx, &slot, &path, want);
            }
        }
        let mut used = Vec::new();
        for j in 0..self.panels.len() {
            let Some(cfg) = self.config_of(j).cloned() else { continue };
            let (rect, scale) = (self.panels[j].rect, self.panels[j].scale);
            used.extend(self.wall_wants(&cfg, rect, scale).iter().map(wall_slot));
        }
        self.bg_images.retain_walls(&used);
    }

    /// Deselects panel `i`'s items, redrawing it if any were selected.
    pub fn clear_selection(&mut self, i: usize) {
        let panel = &mut self.panels[i];
        if panel.selected.is_empty() {
            return;
        }
        panel.selected.clear();
        panel.anchor = None;
        self.paint(i);
    }

    /// Deselects the items of every panel but `i`.
    pub fn clear_other_selections(&mut self, i: usize) {
        for j in (0..self.panels.len()).filter(|&j| j != i) {
            self.clear_selection(j);
        }
    }

    /// Gives panel `i` the look of panel `from` (an id).
    pub fn copy_look(&mut self, i: usize, from: &str) {
        let Some(from) = self.store.panel(from).cloned() else { return };
        // Its old picture is no use now; the new one (if any) loads on the next paint.
        let id = self.panels[i].id.clone();
        self.bg_images.forget(&id);
        self.update_panel(i, |p| *p = with_look_of(p, &from));
    }

    /// The other panels, as (id, title) for a "Copy look from" menu.
    pub fn other_panels(&self, id: &str) -> Vec<(String, String)> {
        let c = self.store.config();
        c.panels
            .iter()
            .filter(|p| p.id != id)
            .map(|p| {
                let title = if p.title.is_empty() { "(untitled)".into() } else { p.title.clone() };
                (p.id.clone(), title)
            })
            .collect()
    }

    /// Opens a new panel that looks like panel `i`, a little below and right of it.
    pub fn duplicate_panel(&mut self, i: usize) {
        let Some(src) = self.config_of(i).cloned() else { return };
        let b = src.bounds;
        let bounds = Bounds::new(b.x + 32, b.y + 32, b.width, b.height);
        let mut panel = duplicate_panel(&src, bounds, "");
        panel.folder_path = self.unique_folder(&panel.title).to_string_lossy().into_owned();
        let id = panel.id.clone();
        self.store.update(|c| c.panels.push(panel));
        self.changed();
        self.open_panel(&id);
    }

    fn open_panel(&mut self, id: &str) {
        let areas = self.work_areas();
        let Some(p) = self.store.panel(id) else { return };
        let clamped = clamp_to_work_areas(p.bounds, &areas, p.collapse_direction, 40);
        if clamped != p.bounds {
            self.store.update_panel(id, |p| p.bounds = clamped);
            self.changed();
        }
        let p = self.store.panel(id).expect("still there").clone();
        let _ = std::fs::create_dir_all(&p.folder_path);
        let title = Wide::new(&p.title);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                PANEL_CLASS,
                title.pcwstr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(hinstance()),
                None,
            )
        };
        let Ok(hwnd) = hwnd else { return };
        let mut panel = Panel::new(id.to_string(), hwnd);
        panel.watch = DirWatch::new(Path::new(&p.folder_path));
        self.panels.push(panel);
        let i = self.panels.len() - 1;
        self.refresh_items(i);
        self.paint(i);
        if !self.hidden {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
        desktop_layer::attach(hwnd);
        procs::register_drop_target(hwnd);
    }

    fn close_panel_window(&mut self, i: usize) {
        let p = self.panels.remove(i);
        dragdrop::revoke(p.hwnd);
        unsafe {
            let _ = DestroyWindow(p.hwnd);
        }
    }

    pub fn quit(&mut self) {
        self.edit = None;
        self.catcher = None;
        for s in self.settings.drain(..) {
            unsafe {
                let _ = DestroyWindow(s.hwnd);
            }
        }
        while !self.panels.is_empty() {
            self.close_panel_window(0);
        }
        self.tray = None;
        self.save_now();
        self.defer(Deferred::Quit);
    }

    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        for p in &self.panels {
            unsafe {
                let _ = ShowWindow(p.hwnd, if hidden { SW_HIDE } else { SW_SHOWNOACTIVATE });
            }
            if !hidden {
                desktop_layer::attach(p.hwnd);
            }
        }
        if hidden {
            // Nothing is shown, so nothing needs to be kept for drawing.
            self.icons.clear();
            self.bg_images.clear();
            self.gfx.trim();
        } else {
            self.paint_all();
        }
    }

    pub fn set_locked(&mut self, locked: bool) {
        self.store.update(|c| c.locked = locked);
        self.changed();
        self.paint_all();
    }

    /// Displays changed: re-read them, pull unreachable panels back and redraw.
    /// Also after other setting changes, such as a new wallpaper: panels showing
    /// it read it again (the old piece shows until the new one is ready).
    pub fn displays_changed(&mut self) {
        self.mons = dpi::monitors();
        self.walls = None;
        let areas = self.work_areas();
        let fixes: Vec<(String, Bounds)> = self
            .store
            .config()
            .panels
            .iter()
            .filter_map(|p| {
                let b = clamp_to_work_areas(p.bounds, &areas, p.collapse_direction, 40);
                (b != p.bounds).then(|| (p.id.clone(), b))
            })
            .collect();
        for (id, b) in fixes {
            self.store.update_panel(&id, |p| p.bounds = b);
        }
        self.changed();
        self.paint_all();
    }

    /// Explorer restarted: the tray icon is gone and Progman is new.
    pub fn explorer_restarted(&mut self) {
        if let Some(t) = &self.tray {
            t.add();
        }
        for p in &self.panels {
            if desktop_layer::needs_reattach(p.hwnd) {
                desktop_layer::attach(p.hwnd);
            }
        }
    }

    // ------------------------------------------------------------- drawing

    pub fn paint_all(&mut self) {
        for i in 0..self.panels.len() {
            self.paint(i);
        }
    }

    /// Where panel `i`'s window is right now, in DIPs: mid-drag, mid-animation
    /// or where its config puts it.
    fn current_bounds(&self, i: usize, cfg: &PanelConfig) -> Bounds {
        let panel = &self.panels[i];
        if let Some(s) = &panel.pointer {
            return s.last;
        }
        if let Some(a) = &panel.anim {
            return a.at(now_ms());
        }
        window_bounds(cfg)
    }

    /// Draws panel `i`.
    pub fn paint(&mut self, i: usize) {
        if self.hidden {
            return;
        }
        let Some(mut cfg) = self.config_of(i).cloned() else { return };
        let bounds = self.current_bounds(i, &cfg);
        if self.panels[i].anim.as_ref().is_some_and(|a| a.expanded_look) {
            // While collapsing or expanding, the panel shows its expanded
            // look, rolled up or out by the window's size.
            cfg.collapsed = false;
        }
        let (rect, scale) = dpi::to_physical(bounds, &self.mons);
        {
            let panel = &mut self.panels[i];
            panel.rect = rect;
            panel.scale = scale;
        }
        let title = if cfg.title.is_empty() { "Untitled" } else { &cfg.title };
        self.panels[i].title_len = self.gfx.measure(title, Font::Title, 10_000.0).0;
        let want = bg_want(&cfg, scale);
        let (walls, walls_fresh) = self.wall_images(&cfg, rect, scale);
        let App { gfx, icons, panels, bg_images, .. } = self;
        let panel = &panels[i];
        let (image, fresh) = match &want {
            Some(w) => bg_images.get(gfx, &panel.id, w),
            None => (None, true),
        };
        // A picture loads again at the new size once a resize is over, not on
        // every step of it. A monitor's wallpaper doesn't depend on the panel.
        let image_missing = (!fresh && panel.pointer.is_none()) || !walls_fresh;
        let l = current_layout(&cfg, panel);
        let cols = l.grid.map_or(1, |g| g.cols);
        let mut cells = cells_of(&cfg, &panel.items, cols);
        // While a drop can land in the grid, show where everything would go.
        const SLOT: &str = "\0slot";
        let mut slot = None;
        let mut hidden_item = None;
        if let Some(DropState { slot: Some(target), own, .. }) = &panel.drop {
            let key = own.clone().unwrap_or_else(|| SLOT.to_string());
            let preview = place_at(&cells, std::slice::from_ref(&key), *target, cols);
            cells = preview
                .iter()
                .map(|p| (p.name.clone(), p.row as usize * cols + p.col as usize))
                .collect();
            slot = cells.get(&key).copied();
            hidden_item = own.as_deref();
        }
        let scene = Scene {
            config: &cfg,
            panel,
            layout: &l,
            drop_active: panel.drop.is_some(),
            cells: &cells,
            slot,
            hidden_item,
            image: image.as_ref(),
            wallpaper: &walls,
        };
        let mut missing = image_missing;
        let hwnd = panel.hwnd;
        if let Err(e) =
            gfx.present(hwnd, rect, scale, |c| panel::draw(c, &scene, icons, &mut missing))
        {
            eprintln!("[render] {e}");
        }
        let panel = &mut panels[i];
        if missing && !panel.icons_queued {
            panel.icons_queued = true;
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_APP_ICONS, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// Loads icons panel `i` is missing, for a little while, then redraws.
    pub fn load_icons(&mut self, i: usize) {
        self.panels[i].icons_queued = false;
        let Some(cfg) = self.config_of(i).cloned() else { return };
        let panel = &self.panels[i];
        let l = current_layout(&cfg, panel);
        // Icons are made at the size they show: the grid's, or a collapsed panel's apps.
        let dip = match l.grid {
            Some(g) => g.icon,
            None => sorter::panel_layout::PEEK_ICON,
        };
        let px = (dip * panel.scale).round() as i32;
        // The items in view: grid tiles on screen, or the apps a collapsed panel shows.
        let visible: Vec<(PathBuf, f64)> = match &l.grid {
            Some(g) => {
                let cells = cells_of(&cfg, &panel.items, g.cols);
                panel
                    .items
                    .iter()
                    .filter(|it| {
                        cells.get(&it.file_name).is_some_and(|&c| {
                            let t = g.tile(c, panel.scroll);
                            t.bottom() >= g.area.y && t.y <= g.area.bottom()
                        })
                    })
                    .map(|it| (it.path.clone(), it.modified_ms))
                    .collect()
            }
            None => panel
                .items
                .iter()
                .take(l.peek.len())
                .map(|it| (it.path.clone(), it.modified_ms))
                .collect(),
        };
        let (scale, id, resting) = (panel.scale, panel.id.clone(), panel.pointer.is_none());
        if let Some(want) = bg_want(&cfg, scale)
            && resting
            && !self.bg_images.get(&self.gfx, &id, &want).1
        {
            self.bg_images.load(&self.gfx, &id, &cfg.bg_image, want);
        }
        self.load_walls(i);
        let start = now_ms();
        for (path, mtime) in visible {
            if now_ms() - start > ICON_BUDGET_MS {
                break;
            }
            if matches!(self.icons.get(&self.gfx, &path, mtime, px), Lookup::Missing) {
                self.icons.load(&self.gfx, &path, mtime, px);
            }
        }
        // Draws again; anything still missing queues another round.
        self.paint(i);
    }

    // ----------------------------------------------------------- animation

    /// Slides or resizes panel `i`'s window from `from` to where its config
    /// puts it, over the collapse animation's length.
    fn animate(&mut self, i: usize, from: Bounds, expanded_look: bool) {
        let Some(cfg) = self.config_of(i).cloned() else { return };
        let to = window_bounds(&cfg);
        if from == to || !animations_on() || self.hidden {
            self.panels[i].anim = None;
            self.paint(i);
            return;
        }
        self.panels[i].anim = Some(Anim { from, to, start: now_ms(), expanded_look });
        unsafe { SetTimer(Some(self.panels[i].hwnd), TIMER_ANIM, FRAME_MS, None) };
        self.paint(i);
    }

    pub fn anim_tick(&mut self, i: usize) {
        let done = self.panels[i].anim.as_ref().is_none_or(|a| a.done(now_ms()));
        if done {
            self.panels[i].anim = None;
            unsafe {
                let _ = KillTimer(Some(self.panels[i].hwnd), TIMER_ANIM);
            }
            self.paint(i);
            // The window can be deeper than the bar; keep it below its
            // neighbours so its transparent part doesn't take their clicks.
            desktop_layer::send_to_bottom(self.panels[i].hwnd);
        } else {
            self.paint(i);
        }
    }

    /// Jumps a running animation to its end (before a move or resize starts).
    pub fn finish_anim(&mut self, i: usize) {
        if self.panels[i].anim.take().is_some() {
            unsafe {
                let _ = KillTimer(Some(self.panels[i].hwnd), TIMER_ANIM);
            }
            self.paint(i);
        }
    }

    // ------------------------------------------------------- panel changes

    /// Changes panel `i`'s config and follows up on what changed: redraws,
    /// relists for a new sort, renames the window.
    pub fn update_panel(&mut self, i: usize, f: impl FnOnce(&mut PanelConfig)) {
        let id = self.panels[i].id.clone();
        let Some(before) = self.store.panel(&id).cloned() else { return };
        self.store.update_panel(&id, f);
        self.changed();
        let after = self.store.panel(&id).expect("updated").clone();
        if after.title != before.title {
            let t = Wide::new(&after.title);
            unsafe {
                let _ = SetWindowTextW(self.panels[i].hwnd, t.pcwstr());
            }
        }
        if after.sort_by != before.sort_by {
            self.refresh_items(i);
        }
        if after.icon_size != before.icon_size {
            self.panels[i].scroll = 0.0;
        }
        self.paint(i);
    }

    pub fn toggle_collapsed(&mut self, i: usize) {
        let Some(before) = self.config_of(i).cloned() else { return };
        let from = self.current_bounds(i, &before);
        self.panels[i].pointer = None;
        self.panels[i].scroll = 0.0;
        self.store.update_panel(&before.id, |p| p.collapsed = !p.collapsed);
        self.changed();
        let after = self.config_of(i).expect("still there").clone();
        self.animate(i, from, true);
        if self.store.config().stacking {
            self.move_stack(&before, &after);
        }
    }

    /// Moves the panels stacked against a panel that collapsed or expanded.
    fn move_stack(&mut self, before: &PanelConfig, after: &PanelConfig) {
        let m = stack_move(before, after, &self.store.config().panels);
        // Leave alone a panel the user is dragging right now.
        let ids: Vec<String> = m
            .ids
            .into_iter()
            .filter(|id| self.panel_by_id(id).is_none_or(|i| self.panels[i].pointer.is_none()))
            .collect();
        if ids.is_empty() {
            return;
        }
        let froms: Vec<(usize, Bounds)> = ids
            .iter()
            .filter_map(|id| {
                let i = self.panel_by_id(id)?;
                let cfg = self.store.panel(id)?.clone();
                Some((i, self.current_bounds(i, &cfg)))
            })
            .collect();
        self.store.update(|c| {
            for p in c.panels.iter_mut().filter(|p| ids.contains(&p.id)) {
                p.bounds.x += m.dx;
                p.bounds.y += m.dy;
            }
        });
        self.changed();
        for (i, from) in froms {
            self.animate(i, from, false);
        }
    }

    // --------------------------------------------------------------- items

    /// Lists panel `i`'s folder again. In custom order, remembers the order
    /// new items arrived in and forgets removed ones.
    pub fn refresh_items(&mut self, i: usize) {
        let Some(cfg) = self.config_of(i).cloned() else { return };
        let items =
            match list_items(Path::new(&cfg.folder_path), cfg.sort_by, &cfg.order, &cfg.cells) {
                Ok(items) => items,
                Err(e) => {
                    eprintln!("[panels] listing failed: {e}");
                    Vec::new()
                }
            };
        if cfg.sort_by == SortBy::Custom {
            let names: Vec<String> = items.iter().map(|i| i.file_name.clone()).collect();
            let order = sync_order(&cfg.order, &names);
            let cells = prune_cells(&cfg.cells, &names);
            if order.is_some() || cells.is_some() {
                self.store.update_panel(&cfg.id, |p| {
                    if let Some(o) = order {
                        p.order = o;
                    }
                    if let Some(c) = cells {
                        p.cells = c;
                    }
                });
                self.changed();
            }
        }
        let panel = &mut self.panels[i];
        let present: std::collections::HashSet<String> =
            items.iter().map(|it| it.file_name.to_lowercase()).collect();
        panel.selected.retain(|s| present.contains(s));
        panel.items = items;
    }

    fn refresh_folder(&mut self, dir: &Path) {
        let hit: Vec<usize> = (0..self.panels.len())
            .filter(|&i| {
                self.config_of(i).is_some_and(|p| same_path(Path::new(&p.folder_path), dir))
            })
            .collect();
        for i in hit {
            self.refresh_items(i);
            self.paint(i);
        }
    }

    /// Paths inside any panel's folder are ones the app may act on.
    pub fn is_managed_path(&self, path: &Path) -> bool {
        self.store.config().panels.iter().any(|p| {
            let dir = Path::new(&p.folder_path);
            folder::is_inside(path, dir) && !same_path(path, dir)
        })
    }

    pub fn open_item(&mut self, path: &Path) {
        if self.is_managed_path(path) {
            self.defer(Deferred::Open(path.to_path_buf()));
        }
    }

    /// Moves (or copies, where `copy` says so) items into `dest`; returns
    /// their new paths. Errors are collected into one message.
    fn move_all(
        &mut self,
        paths: &[PathBuf],
        dest: &Path,
        copy: impl Fn(&Path) -> bool,
    ) -> Vec<PathBuf> {
        let mut errors = Vec::new();
        let mut moved = Vec::new();
        let mut any_move = false;
        let to_desktop = same_path(dest, &self.desktop_dir);
        for src in paths {
            let c = copy(src);
            any_move |= !c;
            match if c { copy_into(src, dest) } else { move_into(src, dest) } {
                Ok(d) => {
                    // Don't let the catching panel take back what the user sent to the desktop.
                    if to_desktop {
                        self.ignore_on_desktop(&d, catcher::IGNORE_MS);
                    }
                    moved.push(d)
                }
                Err(e) => errors.push(format!("{}\n  {e}", src.display())),
            }
        }
        if !errors.is_empty() {
            let (title, extra) = if any_move {
                (
                    "Some items could not be moved",
                    "\n\nItems on the shared Public Desktop need administrator rights to move.",
                )
            } else {
                ("Some items could not be copied", "")
            };
            self.error(title, &format!("{}{extra}", errors.join("\n\n")));
        }
        moved
    }

    /// Files dropped from Explorer, the desktop or another panel: copy or
    /// move them (as `AddMode` says) into the panel's folder, into cell `at`
    /// of a grid `cols` wide, or after the last item. Items from another
    /// panel always move.
    pub fn drop_files(&mut self, i: usize, paths: Vec<PathBuf>, at: Option<(usize, usize)>) {
        let Some(p) = self.config_of(i).cloned() else { return };
        let folders: Vec<PathBuf> =
            self.store.config().panels.iter().map(|o| PathBuf::from(&o.folder_path)).collect();
        let movable: Vec<PathBuf> = paths
            .into_iter()
            .filter(|x| {
                x.exists()
                    && x.parent().is_some() // drive roots
                    && !same_path(x, &self.desktop_dir)
                    && !folders.iter().any(|f| same_path(x, f))
            })
            .collect();
        let copy = self.store.config().add_mode == AddMode::Copy;
        let dest = PathBuf::from(&p.folder_path);
        let managed: Vec<bool> = movable.iter().map(|m| self.is_managed_path(m)).collect();
        let moved = self.move_all(&movable, &dest, |src| {
            copy && !movable.iter().zip(&managed).any(|(m, &managed)| m == src && managed)
        });
        // Without a spot to go to, items that were already here stay where they are.
        let placed: Vec<&PathBuf> = if at.is_some() {
            moved.iter().collect()
        } else {
            moved.iter().filter(|m| !movable.iter().any(|src| same_path(src, m))).collect()
        };
        if placed.is_empty() {
            return;
        }
        let names: Vec<String> = placed
            .iter()
            .filter_map(|m| m.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        let mut cells: Option<Vec<PlacedItem>> = None;
        if let Some((cell, cols)) = at
            && p.sort_by == SortBy::Custom
        {
            // Lay the grid out as the panel shows it, then put the new items in.
            let shown: Vec<String> = list_items(&dest, SortBy::Custom, &p.order, &p.cells)
                .unwrap_or_default()
                .into_iter()
                .map(|it| it.file_name)
                .filter(|n| !names.contains(n))
                .collect();
            let mut c = place_at(&layout_grid(&shown, &p.cells, cols), &names, cell, cols);
            c.truncate(MAX_PANEL_ITEMS);
            cells = Some(c);
        }
        self.store.update_panel(&p.id, |d| {
            d.order = place_in_order(&d.order, &names, None);
            if let Some(c) = cells {
                d.cells = c;
            }
        });
        self.changed();
        // Items already in this folder don't change it, so nothing else refreshes.
        self.refresh_items(i);
        self.paint(i);
    }

    /// Saves where the user put every item; arranging switches to custom order.
    pub fn arrange_items(&mut self, i: usize, cells: Vec<PlacedItem>) {
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<PlacedItem> =
            cells.into_iter().filter(|p| seen.insert(p.name.to_lowercase())).collect();
        self.update_panel(i, |d| {
            d.cells = unique;
            d.sort_by = SortBy::Custom;
        });
        self.refresh_items(i);
        self.paint(i);
    }

    /// Where each item of panel `i` sits, as spots, for a grid `cols` wide.
    pub fn grid_layout(&self, i: usize, cols: usize) -> HashMap<String, usize> {
        match self.config_of(i) {
            Some(cfg) => cells_of(cfg, &self.panels[i].items, cols),
            None => HashMap::new(),
        }
    }

    pub fn move_item_to(&mut self, path: &Path, dest: &Path) {
        if self.is_managed_path(path) {
            self.move_all(&[path.to_path_buf()], dest, |_| false);
            self.refresh_folder(dest);
            if let Some(parent) = path.parent() {
                self.refresh_folder(parent);
            }
        }
    }

    /// Makes a "New folder" in panel `i` and starts renaming it (Ctrl+Shift+N).
    pub fn new_folder(&mut self, i: usize) {
        let Some(cfg) = self.config_of(i).cloned() else { return };
        let made = folder::unique_destination(Path::new(&cfg.folder_path), "New folder")
            .and_then(|p| std::fs::create_dir(&p).map(|_| p));
        match made {
            Ok(path) => {
                self.refresh_items(i);
                let k = file_name(&path).to_lowercase();
                let panel = &mut self.panels[i];
                panel.selected = [k.clone()].into();
                (panel.focus, panel.anchor) = (Some(k.clone()), Some(k));
                self.paint(i);
                self.start_edit(i, EditTarget::Item(path));
            }
            Err(e) => self.error("Could not create a folder", &e.to_string()),
        }
    }

    // -------------------------------------------------------------- rename

    pub fn start_edit(&mut self, i: usize, target: EditTarget) {
        self.edit = None;
        let Some(cfg) = self.config_of(i).cloned() else { return };
        let panel = &self.panels[i];
        let l = current_layout(&cfg, panel);
        let s = panel.scale;
        let screen = |r: sorter::panel_layout::Rect| RECT {
            left: panel.rect.left + (r.x * s).round() as i32,
            top: panel.rect.top + (r.y * s).round() as i32,
            right: panel.rect.left + (r.right() * s).round() as i32,
            bottom: panel.rect.top + (r.bottom() * s).round() as i32,
        };
        let (rect, text, bold, px) = match &target {
            EditTarget::Title(_) => {
                let r = if l.vertical_bar() {
                    let w = l.panel.w.clamp(120.0, 220.0);
                    sorter::panel_layout::Rect::new(0.0, 3.0, w, 24.0)
                } else {
                    sorter::panel_layout::Rect::new(l.title.x, l.bar.y + 3.0, l.title.w, 24.0)
                };
                (screen(r), cfg.title.clone(), true, 12.5)
            }
            EditTarget::Item(path) => {
                let Some(g) = l.grid else { return };
                let Some(item) = panel.items.iter().find(|it| &it.path == path) else { return };
                let cols = g.cols;
                let Some(&cell) = cells_of(&cfg, &panel.items, cols).get(&item.file_name) else {
                    return;
                };
                let t = g.tile(cell, panel.scroll);
                let y = t.y + 7.0 + g.icon + 2.0;
                let r = sorter::panel_layout::Rect::new(t.x - 6.0, y, t.w + 12.0, 22.0);
                (screen(r), item.display_name.clone(), false, 11.5)
            }
        };
        let done = Done { to: self.host, msg: WM_APP_EDIT_DONE };
        let font_px = (px * s).round() as i32;
        if let Some(edit) = EditBox::new(panel.hwnd, rect, &text, font_px, bold, done) {
            self.edit = Some((edit, target));
        }
    }

    pub fn finish_edit(&mut self, keep: bool) {
        let Some((edit, target)) = self.edit.take() else { return };
        let text = edit.text();
        drop(edit);
        if !keep {
            return;
        }
        match target {
            EditTarget::Title(id) => {
                let title = text.trim().to_string();
                let Some(i) = self.panel_by_id(&id) else { return };
                if !title.is_empty() && self.config_of(i).is_some_and(|p| p.title != title) {
                    let title: String = title.chars().take(200).collect();
                    self.update_panel(i, |p| p.title = title);
                }
            }
            EditTarget::Item(path) => {
                let name = text.trim();
                let current =
                    path.file_name().map(|n| folder::display_name_for(&n.to_string_lossy()));
                if name.is_empty()
                    || current.as_deref() == Some(name)
                    || !self.is_managed_path(&path)
                {
                    return;
                }
                match rename_item(&path, name) {
                    Ok(dest) => {
                        let (from, to) = (file_name(&path), file_name(&dest));
                        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                        // Keep the renamed item where the user arranged it.
                        if let Some(i) = (0..self.panels.len()).find(|&i| {
                            self.config_of(i)
                                .is_some_and(|p| same_path(Path::new(&p.folder_path), &dir))
                        }) {
                            self.update_panel(i, |d| {
                                d.order = rename_in_order(&d.order, &from, &to);
                                d.cells = rename_in_cells(&d.cells, &from, &to);
                            });
                            let panel = &mut self.panels[i];
                            if panel.selected.remove(&from.to_lowercase()) {
                                panel.selected.insert(to.to_lowercase());
                            }
                        }
                        self.refresh_folder(&dir);
                    }
                    Err(e) => self.error("Could not rename item", &e.to_string()),
                }
            }
        }
    }

    // ---------------------------------------------------- waits and signals

    /// The handles the main loop waits on besides messages.
    pub fn wait_handles(&self) -> Vec<HANDLE> {
        let panels = self.panels.iter().filter_map(|p| p.watch.as_ref().map(DirWatch::event));
        panels.chain(self.catcher.as_ref().map(|(_, w)| w.event())).collect()
    }

    pub fn on_signal(&mut self, h: HANDLE) {
        if self.catcher.as_ref().is_some_and(|(_, w)| w.event() == h) {
            self.desktop_changed();
            return;
        }
        let Some(i) =
            self.panels.iter().position(|p| p.watch.as_ref().is_some_and(|w| w.event() == h))
        else {
            return;
        };
        let watch = self.panels[i].watch.as_mut().expect("found by its watch");
        if let Report::Failed = watch.read() {
            // The folder was deleted or can't be read: stop watching, show what's left.
            self.panels[i].watch = None;
        }
        unsafe { SetTimer(Some(self.panels[i].hwnd), TIMER_REFRESH, REFRESH_MS, None) };
    }

    // ------------------------------------------------------- app settings

    pub fn set_start_with_windows(&mut self, on: bool) {
        shell::set_start_with_windows(on);
        self.store.update(|c| c.start_with_windows = on);
        self.changed();
    }

    /// Opens panel `i`'s settings window.
    pub fn open_settings(&mut self, i: usize) {
        self.open_settings_window(Some(i));
    }

    pub fn open_app_settings(&mut self) {
        self.open_settings_window(None);
    }

    /// WM_SETTINGCHANGE: the work area or the theme may have changed.
    pub fn settings_changed(&mut self, l: LPARAM) {
        let area = if l.0 == 0 {
            String::new()
        } else {
            unsafe { PCWSTR(l.0 as *const u16).to_string() }.unwrap_or_default()
        };
        if area == "ImmersiveColorSet" {
            self.theme_changed();
        } else {
            self.displays_changed();
        }
    }

    // ------------------------------------------------- catching new items

    /// Turns catching new desktop items on for panel `i` (and off for every
    /// other panel: only one catches), or off.
    pub fn set_catching(&mut self, i: usize, on: bool) {
        let id = self.panels[i].id.clone();
        self.store.update(|c| {
            for p in &mut c.panels {
                p.catch_new_items = on && p.id == id;
            }
        });
        self.changed();
        self.sync_catcher();
    }

    /// Watches the desktop only while a panel catches new items.
    fn sync_catcher(&mut self) {
        let catching = self.store.config().panels.iter().any(|p| p.catch_new_items);
        if catching && self.catcher.is_none() {
            if let Some(watch) = DirWatch::new(&self.desktop_dir) {
                let c = Catcher::new(self.desktop_dir.clone(), catcher::SETTLE_MS);
                self.catcher = Some((c, watch));
            }
        } else if !catching && self.catcher.is_some() {
            self.catcher = None;
            unsafe {
                let _ = KillTimer(Some(self.host), TIMER_CATCH);
            }
        }
    }

    /// Something changed on the desktop: tell the catcher, and check items
    /// once they've had time to settle.
    fn desktop_changed(&mut self) {
        let now = now_ms();
        let Some((catcher, watch)) = self.catcher.as_mut() else { return };
        match watch.read() {
            Report::Changes(changes) => {
                for change in changes {
                    catcher.saw(change, now);
                }
            }
            Report::Failed => {
                self.catcher = None;
                return;
            }
        }
        self.schedule_catch();
    }

    fn schedule_catch(&mut self) {
        let Some(due) = self.catcher.as_ref().and_then(|(c, _)| c.next_due()) else { return };
        let wait = due.saturating_sub(now_ms()).clamp(10, u32::MAX as u64) as u32;
        unsafe { SetTimer(Some(self.host), TIMER_CATCH, wait, None) };
    }

    pub fn host_timer(&mut self, id: usize) {
        if id != TIMER_CATCH {
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.host), TIMER_CATCH);
        }
        let Some((mut catcher, watch)) = self.catcher.take() else { return };
        catcher.run_due(now_ms(), |path| self.catch_item(path));
        self.catcher = Some((catcher, watch));
        self.schedule_catch();
    }

    /// Copies or moves (see `AddMode`) a new desktop item to the end of the
    /// catching panel; false when that failed and is worth another try.
    fn catch_item(&mut self, path: &Path) -> bool {
        let Some(p) = self.store.config().panels.iter().find(|p| p.catch_new_items).cloned() else {
            return true;
        };
        // A panel folder the user put on the desktop is not a new item.
        if self
            .store
            .config()
            .panels
            .iter()
            .any(|x| folder::is_inside(Path::new(&x.folder_path), path))
        {
            return true;
        }
        let dest = Path::new(&p.folder_path);
        let added = match self.store.config().add_mode {
            AddMode::Copy => copy_into(path, dest),
            AddMode::Move => move_into(path, dest),
        };
        match added {
            Ok(d) => {
                let name = file_name(&d);
                self.store
                    .update_panel(&p.id, |x| x.order = place_in_order(&x.order, &[name], None));
                self.changed();
                self.refresh_folder(dest);
                true
            }
            // Locked, or needs admin rights; the catcher may try again.
            Err(e) => {
                eprintln!("[catcher] could not add {}: {e}", path.display());
                false
            }
        }
    }

    /// Keeps the catching panel from taking an item the app itself puts on
    /// the desktop, for `ms`.
    pub fn ignore_on_desktop(&mut self, path: &Path, ms: u64) {
        if let Some((c, _)) = self.catcher.as_mut() {
            c.ignore(&file_name(path), ms, now_ms());
        }
    }

    // --------------------------------------------------------- delete panel

    fn delete_panel(&mut self, id: &str, move_back: bool) {
        let Some(i) = self.panel_by_id(id) else { return };
        let Some(p) = self.config_of(i).cloned() else { return };
        let folder = PathBuf::from(&p.folder_path);
        if move_back {
            let items: Vec<PathBuf> =
                self.panels[i].items.iter().map(|it| it.path.clone()).collect();
            let desktop = self.desktop_dir.clone();
            self.move_all(&items, &desktop, |_| false);
        }
        // Only removes the folder when it's empty; never deletes user files.
        let _ = std::fs::remove_dir(&folder);
        self.store.update(|c| c.panels.retain(|x| x.id != id));
        self.changed();
        self.close_settings_for(Some(id));
        self.bg_images.forget(id);
        self.close_panel_window(i);
        self.sync_catcher();
    }
}

/// What panel `p`'s background image should be decoded for: its file at the
/// panel's expanded size (so collapsing doesn't reload it). None without an image.
fn bg_want(p: &PanelConfig, scale: f32) -> Option<Want> {
    if p.bg_image.is_empty() {
        return None;
    }
    let px = |dip: i32| (dip as f32 * scale).round().max(1.0) as u32;
    let target = (px(p.bounds.width), px(p.bounds.height));
    Some(effect_want(p, Want::picture(&p.bg_image, p.bg_image_fit, target, scale), scale))
}

/// `want`, blurred for the blur effect or (a wallpaper piece) made smaller for pixelate.
fn effect_want(p: &PanelConfig, want: Want, scale: f32) -> Want {
    match p.bg_effect {
        BgEffect::Blur => {
            let radius = blur_radius(p.bg_effect_strength) * scale;
            want.blurred(radius.round() as u32, blur_reduce(radius))
        }
        BgEffect::Pixelate if matches!(want.kind, Kind::Wallpaper { .. }) => {
            let block = pixel_block(p.bg_effect_strength) * scale;
            // A pixel per block at most: smaller than the blocks would be wasted.
            want.reduced((block as u32).clamp(2, 12))
        }
        _ => want,
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Asks how to delete a panel (moving its items back to the desktop, or
/// keeping its folder), then does it.
fn delete_panel_flow(id: &str) {
    let Some((hwnd, title, folder, count)) = with_app(|a| {
        let i = a.panel_by_id(id)?;
        a.refresh_items(i);
        let p = a.config_of(i)?;
        Some((a.panels[i].hwnd, p.title.clone(), p.folder_path.clone(), a.panels[i].items.len()))
    })
    .flatten() else {
        return;
    };
    let move_back = if count == 0 {
        let main = format!("Delete the empty panel \"{title}\"?");
        match dialogs::ask(Some(hwnd), "Delete panel", &main, "", &["Delete", "Cancel"], false) {
            Some(0) => false,
            _ => return,
        }
    } else {
        let main = format!("Delete the panel \"{title}\"?");
        let detail = format!("It contains {count} item(s) stored in:\n{folder}");
        let buttons = ["Move items to Desktop and delete", "Delete panel, keep folder", "Cancel"];
        match dialogs::ask(Some(hwnd), "Delete panel", &main, &detail, &buttons, true) {
            Some(0) => true,
            Some(1) => false,
            _ => return,
        }
    };
    with_app(|a| a.delete_panel(id, move_back));
    run_deferred();
}

/// Lets the user pick one of a panel's colors.
fn choose_color_flow(owner: HWND, panel: &str, layer: Layer) {
    let Some(current) = with_app(|a| {
        a.store.panel(panel).and_then(|p| match layer {
            Layer::Background => Some(p.bg_color.clone()),
            Layer::Bar => Some(p.title_bar_color.clone()),
            Layer::Text => Some(p.title_text_color.clone()),
            Layer::Texture => Some(p.texture_color.clone()),
            Layer::Image | Layer::TextureSize | Layer::Effect => None,
        })
    })
    .flatten() else {
        return;
    };
    let Some(hex) = dialogs::choose_color(Some(owner), &current) else { return };
    with_app(|a| {
        if let Some(i) = a.panel_by_id(panel) {
            a.update_panel(i, |p| match layer {
                Layer::Background => p.bg_color = hex,
                Layer::Bar => p.title_bar_color = hex,
                Layer::Text => p.title_text_color = hex,
                Layer::Texture => p.texture_color = hex,
                Layer::Image | Layer::TextureSize | Layer::Effect => {}
            });
        }
    });
}

/// Lets the user pick a panel's background image.
fn choose_image_flow(owner: HWND, panel: &str) {
    let Some(current) = with_app(|a| a.store.panel(panel).map(|p| p.bg_image.clone())).flatten()
    else {
        return;
    };
    let start = Path::new(&current).parent().map(Path::to_path_buf);
    let Some(file) = dialogs::choose_image(Some(owner), start.as_deref()) else { return };
    with_app(|a| {
        let Some(i) = a.panel_by_id(panel) else { return };
        // Frees the old picture now rather than when the cache fills.
        a.bg_images.forget(panel);
        let path = file.to_string_lossy().into_owned();
        a.update_panel(i, |p| p.bg_image = path);
    });
}

/// Lets the user point a panel at another folder.
fn choose_folder_flow(owner: HWND, panel: &str) {
    let Some((current, desktop)) = with_app(|a| {
        a.store.panel(panel).map(|p| (PathBuf::from(&p.folder_path), a.desktop_dir.clone()))
    })
    .flatten() else {
        return;
    };
    let Some(dir) =
        dialogs::choose_folder(Some(owner), "Choose the folder this panel shows", &current)
    else {
        return;
    };
    if same_path(&dir, &desktop) {
        shell::error_box(
            Some(owner),
            "Choose another folder",
            "A panel cannot show the Desktop folder itself.",
        );
        return;
    }
    with_app(|a| {
        let Some(i) = a.panel_by_id(panel) else { return };
        let folder = dir.to_string_lossy().into_owned();
        a.update_panel(i, |p| p.folder_path = folder);
        a.panels[i].watch = DirWatch::new(&dir);
        a.panels[i].scroll = 0.0;
        a.refresh_items(i);
        a.paint(i);
    });
}
