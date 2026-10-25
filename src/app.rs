//! The app: config, every panel window, the tray and what they do. One
//! instance lives in a thread-local on the UI thread; window procedures reach
//! it through `with_app`.
//!
//! Modal UI (menus, dialogs, drag and drop) runs its own message loop, which
//! calls back into the window procedures. So it never runs while the app is
//! borrowed: handlers queue it as a `Deferred` and it runs once the handler
//! has returned.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use sorter::config::{ConfigStore, SAVE_DEBOUNCE_MS, create_panel, folder_name_from_title};
use sorter::folder::{self, list_items, prune_cells, same_path, sync_order};
use sorter::geometry::clamp_to_work_areas;
use sorter::model::{Bounds, PanelConfig, SortBy};
use sorter::panel_layout::Hit;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_CONTROL, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::panel::{
    self, DRAG_THRESHOLD, Panel, PointerKind, Press, Scene, cells_of, current_layout, end_bounds,
    item_at_cell, pointer_bounds, start_pointer, window_bounds,
};
use crate::win::desktop_layer;
use crate::win::dpi::{self, Monitor};
use crate::win::icons::IconCache;
use crate::win::render::Gfx;
use crate::win::shell;
use crate::win::tray::Tray;
use crate::win::watch::{DirWatch, Report};
use crate::win::{Wide, now_ms, point_of, wheel_delta};

pub const WM_APP_TRAY: u32 = WM_APP + 1;
/// Sent by a second instance: show the panels.
pub const WM_APP_SHOW: u32 = WM_APP + 2;
/// Load icons a panel drew placeholders for.
const WM_APP_ICONS: u32 = WM_APP + 3;

pub const HOST_CLASS: PCWSTR = w!("DesktopIconsSorter.Host");
const PANEL_CLASS: PCWSTR = w!("DesktopIconsSorter.Panel");

const TIMER_SAVE: usize = 1;
const TIMER_REFRESH: usize = 1;
/// How long a burst of folder changes is collected before listing it again.
const REFRESH_MS: u32 = 150;
/// How long loading icons may hold up the UI before it redraws and goes on.
const ICON_BUDGET_MS: u64 = 40;
const WHEEL_STEP: f32 = 48.0;

/// Modal work queued by a handler (see the module docs).
pub enum Deferred {
    Error { title: String, text: String },
    Open(PathBuf),
    TrayMenu,
    Quit,
}

pub struct App {
    pub store: ConfigStore,
    pub gfx: Gfx,
    pub icons: IconCache,
    pub panels: Vec<Panel>,
    pub host: HWND,
    tray: Option<Tray>,
    pub hidden: bool,
    pub desktop_dir: PathBuf,
    pub panels_root: PathBuf,
    pub mons: Vec<Monitor>,
    deferred: VecDeque<Deferred>,
    taskbar_created: u32,
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
        Deferred::TrayMenu => tray_menu(),
        Deferred::Quit => unsafe { PostQuitMessage(0) },
    }
}

fn hinstance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe { GetModuleHandleW(None) }.map(Into::into).unwrap_or_default()
}

pub fn register_classes() {
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let icon = unsafe { LoadIconW(Some(hinstance()), PCWSTR(1 as _)) }.unwrap_or_default();
    let classes: [(PCWSTR, WNDPROC, WNDCLASS_STYLES); 2] = [
        (HOST_CLASS, Some(host_proc), WNDCLASS_STYLES(0)),
        (PANEL_CLASS, Some(panel_proc), CS_DBLCLKS),
    ];
    for (name, proc_, style) in classes {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style,
            lpfnWndProc: proc_,
            hInstance: hinstance(),
            hCursor: cursor,
            hIcon: icon,
            lpszClassName: name,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
    }
}

/// The hidden window that owns the tray icon and app-wide timers. It's a
/// top-level window (not message-only) so it hears Explorer's TaskbarCreated
/// broadcast.
pub fn create_host() -> HWND {
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            HOST_CLASS,
            w!("Desktop Icons Sorter"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance()),
            None,
        )
        .unwrap_or_default()
    }
}

impl App {
    pub fn new(store: ConfigStore, host: HWND) -> windows::core::Result<Self> {
        Ok(Self {
            store,
            gfx: Gfx::new()?,
            icons: IconCache::default(),
            panels: Vec::new(),
            host,
            tray: None,
            hidden: false,
            desktop_dir: shell::desktop_dir(),
            panels_root: sorter::config::panels_root_dir(),
            mons: dpi::monitors(),
            deferred: VecDeque::new(),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        })
    }

    pub fn start(&mut self) {
        self.tray = Some(Tray::new(self.host, WM_APP_TRAY));
        if self.store.config().panels.is_empty() {
            self.add_panel("Apps");
        }
        let ids: Vec<String> = self.store.config().panels.iter().map(|p| p.id.clone()).collect();
        for id in ids {
            self.open_panel(&id);
        }
    }

    pub fn defer(&mut self, d: Deferred) {
        self.deferred.push_back(d);
    }

    pub fn error(&mut self, title: &str, text: &str) {
        self.defer(Deferred::Error { title: title.into(), text: text.into() });
    }

    /// Saves the config a moment after the last change.
    pub fn changed(&mut self) {
        if self.store.is_dirty() {
            unsafe { SetTimer(Some(self.host), TIMER_SAVE, SAVE_DEBOUNCE_MS, None) };
        }
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

    fn panel_index_by_id(&self, id: &str) -> Option<usize> {
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
    }

    pub fn quit(&mut self) {
        for p in self.panels.drain(..) {
            unsafe {
                let _ = DestroyWindow(p.hwnd);
            }
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
            self.gfx.trim();
        } else {
            for i in 0..self.panels.len() {
                self.paint(i);
            }
        }
    }

    pub fn set_locked(&mut self, locked: bool) {
        self.store.update(|c| c.locked = locked);
        self.changed();
    }

    /// Displays changed: re-read them, pull unreachable panels back and redraw.
    pub fn displays_changed(&mut self) {
        self.mons = dpi::monitors();
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
        for i in 0..self.panels.len() {
            self.paint(i);
        }
    }

    /// Explorer restarted: the tray icon is gone and Progman is new.
    fn explorer_restarted(&mut self) {
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

    /// Draws panel `i` where its config puts it (or where a drag has it now).
    pub fn paint(&mut self, i: usize) {
        let Some(cfg) = self.config_of(i).cloned() else { return };
        let bounds =
            self.panels[i].pointer.as_ref().map_or_else(|| window_bounds(&cfg), |s| s.last);
        let (rect, scale) = dpi::to_physical(bounds, &self.mons);
        {
            let panel = &mut self.panels[i];
            panel.rect = rect;
            panel.scale = scale;
        }
        let title = if cfg.title.is_empty() { "Untitled" } else { &cfg.title };
        self.panels[i].title_len =
            self.gfx.measure(title, crate::win::render::Font::Title, 10_000.0).0;
        let locked = self.store.config().locked;
        let App { gfx, icons, panels, .. } = self;
        let panel = &panels[i];
        let l = current_layout(&cfg, panel);
        let cols = l.grid.map_or(1, |g| g.cols);
        let cells = cells_of(&cfg, &panel.items, cols);
        let scene = Scene {
            config: &cfg,
            panel,
            layout: &l,
            locked,
            drop_active: false,
            cells: &cells,
            slot: None,
            hidden_item: None,
        };
        let mut missing = false;
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
    fn load_icons(&mut self, i: usize) {
        self.panels[i].icons_queued = false;
        let Some(cfg) = self.config_of(i).cloned() else { return };
        let panel = &self.panels[i];
        let l = current_layout(&cfg, panel);
        let px = (sorter::model::icon_pixels(cfg.icon_size) as f32 * panel.scale).round() as i32;
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
        let start = now_ms();
        for (path, mtime) in visible {
            if now_ms() - start > ICON_BUDGET_MS {
                break;
            }
            if matches!(
                self.icons.get(&self.gfx, &path, mtime, px),
                crate::win::icons::Lookup::Missing
            ) {
                self.icons.load(&self.gfx, &path, mtime, px);
            }
        }
        // Draws again; anything still missing queues another round.
        self.paint(i);
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

    // ------------------------------------------------------------ collapse

    pub fn toggle_collapsed(&mut self, i: usize) {
        let id = self.panels[i].id.clone();
        self.store.update_panel(&id, |p| p.collapsed = !p.collapsed);
        self.changed();
        self.panels[i].scroll = 0.0;
        self.paint(i);
    }

    // ---------------------------------------------------- waits and timers

    /// The handles the main loop waits on besides messages.
    pub fn wait_handles(&self) -> Vec<HANDLE> {
        self.panels.iter().filter_map(|p| p.watch.as_ref().map(DirWatch::event)).collect()
    }

    pub fn on_signal(&mut self, h: HANDLE) {
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
}

// ------------------------------------------------------------- tray menu

const CMD_NEW: usize = 1;
const CMD_SHOW: usize = 2;
const CMD_LOCK: usize = 3;
const CMD_CONFIG: usize = 6;
const CMD_QUIT: usize = 7;

/// A popup menu; items are (id, label, checked), id 0 for a separator.
fn popup(items: &[(usize, &str, bool)]) -> HMENU {
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        for &(id, label, checked) in items {
            if id == 0 {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            } else {
                let flags = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
                let l = Wide::new(label);
                let _ = AppendMenuW(menu, flags, id, l.pcwstr());
            }
        }
        menu
    }
}

/// Shows a menu at the cursor and returns the chosen id (0 when dismissed).
pub fn track(menu: HMENU, owner: HWND) -> usize {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
        // Without this the menu doesn't close when clicking elsewhere.
        let _ = SetForegroundWindow(owner);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            pt.x,
            pt.y,
            None,
            owner,
            None,
        );
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        cmd.0 as usize
    }
}

fn tray_menu() {
    let Some((host, hidden, locked)) = with_app(|a| (a.host, a.hidden, a.store.config().locked))
    else {
        return;
    };
    let menu = popup(&[
        (CMD_NEW, "New panel", false),
        (CMD_SHOW, if hidden { "Show panels" } else { "Hide panels" }, false),
        (CMD_LOCK, "Lock panels", locked),
        (0, "", false),
        (CMD_CONFIG, "Open config folder", false),
        (0, "", false),
        (CMD_QUIT, "Quit", false),
    ]);
    let cmd = track(menu, host);
    with_app(|a| match cmd {
        CMD_NEW => a.new_panel(),
        CMD_SHOW => a.set_hidden(!a.hidden),
        CMD_LOCK => a.set_locked(!a.store.config().locked),
        CMD_CONFIG => {
            if let Some(dir) = a.store.path().parent() {
                a.defer(Deferred::Open(dir.to_path_buf()));
            }
        }
        CMD_QUIT => a.quit(),
        _ => {}
    });
    run_deferred();
}

// --------------------------------------------------------- window procs

extern "system" fn host_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let handled = with_app(|a| {
        if msg == a.taskbar_created && msg != 0 {
            a.explorer_restarted();
            return Some(LRESULT(0));
        }
        match msg {
            WM_APP_TRAY => {
                match (l.0 as u32) & 0xffff {
                    WM_LBUTTONUP => a.set_hidden(!a.hidden),
                    WM_RBUTTONUP | WM_CONTEXTMENU => a.defer(Deferred::TrayMenu),
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_APP_SHOW => {
                a.set_hidden(false);
                Some(LRESULT(0))
            }
            WM_TIMER if w.0 == TIMER_SAVE => {
                a.save_now();
                Some(LRESULT(0))
            }
            WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
                a.displays_changed();
                None
            }
            WM_QUERYENDSESSION => {
                a.save_now();
                Some(LRESULT(1))
            }
            _ => None,
        }
    })
    .flatten();
    run_deferred();
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, w, l) })
}

fn ctrl_down() -> bool {
    (unsafe { GetKeyState(VK_CONTROL.0 as i32) }) < 0
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    p
}

fn moved_past(a: POINT, b: POINT) -> bool {
    (a.x - b.x).abs().max((a.y - b.y).abs()) >= DRAG_THRESHOLD
}

extern "system" fn panel_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_WINDOWPOSCHANGING {
        // Stay at the bottom of the z-order even while active.
        let pos = unsafe { &mut *(l.0 as *mut WINDOWPOS) };
        if !pos.flags.contains(SWP_NOZORDER) {
            pos.hwndInsertAfter = HWND_BOTTOM;
        }
        return LRESULT(0);
    }
    let handled = with_app(|a| {
        let i = a.panel_index(hwnd)?;
        panel_message(a, i, msg, w, l)
    })
    .flatten();
    run_deferred();
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, w, l) })
}

/// The mouse position in DIPs within panel `i`.
fn dip(a: &App, i: usize, l: LPARAM) -> (f32, f32) {
    let (x, y) = point_of(l);
    let s = a.panels[i].scale;
    (x as f32 / s, y as f32 / s)
}

fn panel_message(a: &mut App, i: usize, msg: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    let hwnd = a.panels[i].hwnd;
    let cfg = a.config_of(i)?.clone();
    let locked = a.store.config().locked;
    match msg {
        WM_LBUTTONDOWN => {
            let (x, y) = dip(a, i, l);
            let hit = panel::hit(&cfg, &a.panels[i], locked, x, y);
            let at = cursor();
            let press = match hit {
                Hit::Resize(edge) => {
                    start_move(a, i, PointerKind::Resize(edge));
                    None
                }
                Hit::Chevron | Hit::Gear => Some(Press::Button(hit)),
                Hit::TitleText | Hit::TitleBar => (!locked).then_some(Press::Title(at)),
                Hit::Cell(cell) => {
                    let cols = current_layout(&cfg, &a.panels[i]).grid.map_or(1, |g| g.cols);
                    let item = item_at_cell(&cfg, &a.panels[i].items, cols, cell)
                        .map(|it| it.file_name.clone());
                    let panel = &mut a.panels[i];
                    match item {
                        Some(name) => {
                            let k = name.to_lowercase();
                            if ctrl_down() {
                                if !panel.selected.remove(&k) {
                                    panel.selected.insert(k);
                                }
                            } else if !panel.selected.contains(&k) {
                                panel.selected = [k].into();
                            }
                            Some(Press::Item(at, name))
                        }
                        None => {
                            panel.selected.clear();
                            None
                        }
                    }
                }
                Hit::Peek(n) => {
                    a.panels[i].items.get(n).map(|it| Press::Peek(at, it.file_name.clone()))
                }
                Hit::Body => {
                    a.panels[i].selected.clear();
                    None
                }
            };
            if press.is_some() {
                unsafe { SetCapture(hwnd) };
            }
            a.panels[i].press = press;
            a.paint(i);
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE => {
            if a.panels[i].pointer.is_some() {
                pointer_moved(a, i);
                return Some(LRESULT(0));
            }
            if let Some(Press::Title(at)) = a.panels[i].press
                && moved_past(at, cursor())
            {
                a.panels[i].press = None;
                start_move(a, i, PointerKind::Move);
                return Some(LRESULT(0));
            }
            let (x, y) = dip(a, i, l);
            let hit = panel::hit(&cfg, &a.panels[i], locked, x, y);
            let panel = &mut a.panels[i];
            if !panel.inside {
                let mut t = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                unsafe {
                    let _ = TrackMouseEvent(&mut t);
                }
            }
            if !panel.inside || panel.hover != Some(hit) {
                panel.inside = true;
                panel.hover = Some(hit);
                a.paint(i);
            }
            Some(LRESULT(0))
        }
        WM_MOUSELEAVE => {
            let panel = &mut a.panels[i];
            panel.inside = false;
            panel.hover = None;
            a.paint(i);
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            let press = a.panels[i].press.take();
            if a.panels[i].pointer.is_some() {
                end_move(a, i);
            }
            unsafe {
                let _ = ReleaseCapture();
            }
            let (x, y) = dip(a, i, l);
            let hit = panel::hit(&cfg, &a.panels[i], locked, x, y);
            match press {
                Some(Press::Button(Hit::Chevron)) if hit == Hit::Chevron => a.toggle_collapsed(i),
                Some(Press::Button(Hit::Gear)) if hit == Hit::Gear => {}
                Some(Press::Peek(_, name)) if matches!(hit, Hit::Peek(_)) => {
                    let path = Path::new(&cfg.folder_path).join(name);
                    a.open_item(&path);
                }
                _ => {}
            }
            Some(LRESULT(0))
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = dip(a, i, l);
            match panel::hit(&cfg, &a.panels[i], locked, x, y) {
                Hit::TitleBar | Hit::TitleText => a.toggle_collapsed(i),
                Hit::Cell(cell) => {
                    let cols = current_layout(&cfg, &a.panels[i]).grid.map_or(1, |g| g.cols);
                    if let Some(item) = item_at_cell(&cfg, &a.panels[i].items, cols, cell) {
                        let path = item.path.clone();
                        a.open_item(&path);
                    }
                }
                _ => {}
            }
            Some(LRESULT(0))
        }
        WM_CAPTURECHANGED => {
            a.panels[i].press = None;
            if a.panels[i].pointer.is_some() {
                end_move(a, i);
            }
            Some(LRESULT(0))
        }
        WM_MOUSEWHEEL => {
            let l_ = current_layout(&cfg, &a.panels[i]);
            if let Some(g) = l_.grid {
                let cells = cells_of(&cfg, &a.panels[i].items, g.cols);
                let used = cells.values().max().map_or(0, |m| m + 1);
                let panel = &mut a.panels[i];
                let steps = wheel_delta(w) as f32 / 120.0;
                let next = (panel.scroll - steps * WHEEL_STEP).clamp(0.0, g.max_scroll(used));
                if next != panel.scroll {
                    panel.scroll = next;
                    a.paint(i);
                }
            }
            Some(LRESULT(0))
        }
        WM_SETCURSOR => {
            let p = cursor();
            let r = a.panels[i].rect;
            let s = a.panels[i].scale;
            let (x, y) = ((p.x - r.left) as f32 / s, (p.y - r.top) as f32 / s);
            let id = match panel::hit(&cfg, &a.panels[i], locked, x, y) {
                Hit::Resize(e) if (e.n || e.s) && !(e.e || e.w) => IDC_SIZENS,
                Hit::Resize(e) if !(e.n || e.s) => IDC_SIZEWE,
                Hit::Resize(e) if (e.n && e.e) || (e.s && e.w) => IDC_SIZENESW,
                Hit::Resize(_) => IDC_SIZENWSE,
                _ => IDC_ARROW,
            };
            unsafe { SetCursor(LoadCursorW(None, id).ok()) };
            Some(LRESULT(1))
        }
        WM_KEYDOWN => {
            let key = w.0 as u16;
            let panel = &mut a.panels[i];
            if key == VK_RETURN.0 {
                let paths: Vec<PathBuf> = panel
                    .items
                    .iter()
                    .filter(|it| panel.is_selected(it))
                    .map(|it| it.path.clone())
                    .collect();
                for p in paths {
                    a.open_item(&p);
                }
            } else if key == VK_ESCAPE.0 {
                panel.selected.clear();
                a.paint(i);
            } else if key == b'A' as u16 && ctrl_down() {
                panel.selected = panel.items.iter().map(|it| it.file_name.to_lowercase()).collect();
                a.paint(i);
            }
            Some(LRESULT(0))
        }
        WM_ACTIVATE => {
            // Back to the bottom once another window takes over.
            if (w.0 & 0xffff) as u32 == WA_INACTIVE {
                desktop_layer::send_to_bottom(hwnd);
            }
            None
        }
        WM_TIMER if w.0 == TIMER_REFRESH => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_REFRESH);
            }
            a.refresh_items(i);
            a.paint(i);
            Some(LRESULT(0))
        }
        WM_APP_ICONS => {
            a.load_icons(i);
            Some(LRESULT(0))
        }
        WM_DPICHANGED => {
            a.mons = dpi::monitors();
            a.paint(i);
            Some(LRESULT(0))
        }
        // Alt+F4 on a panel doesn't close it; panels go away from their menu.
        WM_CLOSE => Some(LRESULT(0)),
        _ => None,
    }
}

fn start_move(a: &mut App, i: usize, kind: PointerKind) {
    if a.store.config().locked {
        return;
    }
    let Some(cfg) = a.config_of(i).cloned() else { return };
    let snapping = a.store.config().snapping;
    let scale = a.panels[i].scale;
    let pointer = start_pointer(&cfg, &a.store.config().panels, kind, snapping, scale);
    if pointer.is_some() {
        unsafe { SetCapture(a.panels[i].hwnd) };
    }
    a.panels[i].pointer = pointer;
}

fn pointer_moved(a: &mut App, i: usize) {
    let Some(cfg) = a.config_of(i).cloned() else { return };
    let c = cursor();
    let work = dpi::monitor_for_point(c, &a.mons).dip_work();
    let s = a.panels[i].pointer.as_ref().expect("checked by the caller");
    let next = pointer_bounds(s, &cfg, c, work);
    if next == s.last {
        return;
    }
    let size_changed = (next.width, next.height) != (s.last.width, s.last.height);
    a.panels[i].pointer.as_mut().expect("checked").last = next;
    let (rect, scale) = dpi::to_physical(next, &a.mons);
    let panel = &mut a.panels[i];
    if size_changed || scale != panel.scale {
        a.paint(i);
    } else {
        // Moving only: no need to draw again.
        panel.rect = rect;
        let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
        unsafe {
            let _ = SetWindowPos(panel.hwnd, None, rect.left, rect.top, 0, 0, flags);
        }
    }
}

fn end_move(a: &mut App, i: usize) {
    let Some(s) = a.panels[i].pointer.take() else { return };
    let Some(cfg) = a.config_of(i).cloned() else { return };
    let b = end_bounds(&s, &cfg);
    if b != cfg.bounds {
        a.store.update_panel(&cfg.id, |p| p.bounds = b);
        a.changed();
    }
    a.paint(i);
}

/// For callers outside the window procedures that know a panel by id.
pub fn paint_panel(id: &str) {
    with_app(|a| {
        if let Some(i) = a.panel_index_by_id(id) {
            a.paint(i);
        }
    });
}

#[allow(dead_code)]
fn unused(_: RECT) {}
