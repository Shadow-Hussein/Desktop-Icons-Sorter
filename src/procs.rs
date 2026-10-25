//! Window procedures and input: turning messages into calls on the app.

use std::path::{Path, PathBuf};

use sorter::grid::place_at;
use sorter::model::{AddMode, IconSize, SortBy};
use sorter::panel_layout::Hit;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH, HDC, SetBkColor, SetTextColor};
use windows::Win32::System::Ole::DROPEFFECT;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_CONTROL, VK_ESCAPE, VK_F2, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::app::{
    App, Deferred, EditTarget, HOST_CLASS, MenuKind, PANEL_CLASS, TIMER_ANIM, TIMER_REFRESH,
    TIMER_SAVE, WM_APP_EDIT_DONE, WM_APP_ICONS, WM_APP_SHOW, WM_APP_TRAY, hinstance, run_deferred,
    with_app,
};
use crate::panel::{
    self, DRAG_THRESHOLD, DropState, PointerKind, Press, cells_of, current_layout, end_bounds,
    item_at_cell, pointer_bounds, start_pointer,
};
use crate::win::dragdrop::{self, DropHandler, effect_for};
use crate::win::menu::{self, Item};
use crate::win::{desktop_layer, dpi, point_of, wheel_delta};

const WHEEL_STEP: f32 = 48.0;
/// Rename boxes: white text on near-black, like the old dark inputs.
const EDIT_BG: COLORREF = COLORREF(0x00302420);

fn edit_brush() -> HBRUSH {
    thread_local! {
        static BRUSH: HBRUSH = unsafe { CreateSolidBrush(EDIT_BG) };
    }
    BRUSH.with(|b| *b)
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

// ------------------------------------------------------------------- menus

const CMD_NEW: usize = 1;
const CMD_SHOW: usize = 2;
const CMD_LOCK: usize = 3;
const CMD_START: usize = 4;
const CMD_APP_SETTINGS: usize = 5;
const CMD_CONFIG: usize = 6;
const CMD_QUIT: usize = 7;
const CMD_OPEN: usize = 10;
const CMD_LOCATION: usize = 11;
const CMD_RENAME: usize = 12;
const CMD_TO_DESKTOP: usize = 13;
const CMD_RECYCLE: usize = 14;
const CMD_SIZE: usize = 20; // + IconSize index
const CMD_SORT: usize = 30; // + SortBy index
const CMD_OPEN_FOLDER: usize = 40;
const CMD_SETTINGS: usize = 41;
const CMD_RENAME_TITLE: usize = 43;
const CMD_COLLAPSE: usize = 44;
const CMD_DELETE: usize = 47;
const CMD_TO_PANEL: usize = 100; // + panel index

const SIZES: [(IconSize, &str); 3] = [
    (IconSize::Small, "Small icons"),
    (IconSize::Medium, "Medium icons"),
    (IconSize::Large, "Large icons"),
];
const SORTS: [(SortBy, &str); 4] = [
    (SortBy::Custom, "Custom (drag to arrange)"),
    (SortBy::Name, "Name"),
    (SortBy::Type, "Type"),
    (SortBy::Modified, "Date modified"),
];

pub fn tray_menu() {
    let Some((host, hidden, locked, start)) = with_app(|a| {
        let c = a.store.config();
        (a.host, a.hidden, c.locked, c.start_with_windows)
    }) else {
        return;
    };
    let items = [
        Item::cmd(CMD_NEW, "New panel"),
        Item::cmd(CMD_SHOW, if hidden { "Show panels" } else { "Hide panels" }),
        Item::check(CMD_LOCK, "Lock panels", locked),
        Item::Sep,
        Item::check(CMD_START, "Start with Windows", start),
        Item::cmd(CMD_APP_SETTINGS, "Settings…"),
        Item::cmd(CMD_CONFIG, "Open config folder"),
        Item::Sep,
        Item::cmd(CMD_QUIT, "Quit"),
    ];
    let cmd = menu::show(&items, host);
    with_app(|a| match cmd {
        CMD_NEW => a.new_panel(),
        CMD_SHOW => a.set_hidden(!a.hidden),
        CMD_LOCK => a.set_locked(!a.store.config().locked),
        CMD_START => a.set_start_with_windows(!a.store.config().start_with_windows),
        CMD_APP_SETTINGS => a.open_app_settings(),
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

fn view_menu(size: IconSize, sort: SortBy) -> [Item; 2] {
    [
        Item::Sub(
            "View".into(),
            SIZES
                .iter()
                .enumerate()
                .map(|(n, (v, l))| Item::check(CMD_SIZE + n, l, *v == size))
                .collect(),
        ),
        Item::Sub(
            "Sort by".into(),
            SORTS
                .iter()
                .enumerate()
                .map(|(n, (v, l))| Item::check(CMD_SORT + n, l, *v == sort))
                .collect(),
        ),
    ]
}

/// Shows a panel's title, background or item menu and does what was chosen.
pub fn panel_menu(id: &str, kind: MenuKind) {
    let Some((hwnd, items, others)) = with_app(|a| {
        let i = a.panel_by_id(id)?;
        let p = a.config_of(i)?.clone();
        let others: Vec<(String, String)> = a
            .store
            .config()
            .panels
            .iter()
            .filter(|o| o.id != id)
            .map(|o| {
                (
                    o.id.clone(),
                    if o.title.is_empty() { "(untitled)".into() } else { o.title.clone() },
                )
            })
            .collect();
        let [view, sort] = view_menu(p.icon_size, p.sort_by);
        let items = match &kind {
            MenuKind::Item(_) => {
                let mut to = vec![Item::cmd(CMD_TO_DESKTOP, "Desktop")];
                if !others.is_empty() {
                    to.push(Item::Sep);
                }
                to.extend(
                    others.iter().enumerate().map(|(n, (_, t))| Item::cmd(CMD_TO_PANEL + n, t)),
                );
                vec![
                    Item::cmd(CMD_OPEN, "Open"),
                    Item::cmd(CMD_LOCATION, "Open file location"),
                    Item::cmd(CMD_RENAME, "Rename"),
                    Item::Sep,
                    Item::Sub("Move to".into(), to),
                    Item::Sep,
                    Item::cmd(CMD_RECYCLE, "Move to Recycle Bin"),
                ]
            }
            MenuKind::Background => vec![
                view,
                sort,
                Item::Sep,
                Item::cmd(CMD_OPEN_FOLDER, "Open folder in Explorer"),
                Item::cmd(CMD_SETTINGS, "Panel settings…"),
                Item::Sep,
                Item::cmd(CMD_NEW, "New panel"),
            ],
            MenuKind::Title => vec![
                Item::cmd(CMD_RENAME_TITLE, "Rename"),
                Item::cmd(CMD_COLLAPSE, if p.collapsed { "Expand" } else { "Collapse" }),
                Item::cmd(CMD_SETTINGS, "Panel settings…"),
                view,
                sort,
                Item::Sep,
                Item::cmd(CMD_NEW, "New panel"),
                Item::check(CMD_LOCK, "Lock all panels", a.store.config().locked),
                Item::cmd(CMD_APP_SETTINGS, "App settings…"),
                Item::Sep,
                Item::cmd(CMD_DELETE, "Delete panel…"),
            ],
        };
        Some((a.panels[i].hwnd, items, others))
    })
    .flatten() else {
        return;
    };
    let cmd = menu::show(&items, hwnd);
    with_app(|a| {
        let Some(i) = a.panel_by_id(id) else { return };
        let item = match &kind {
            MenuKind::Item(p) => Some(p.clone()),
            _ => None,
        };
        match cmd {
            CMD_OPEN => item.iter().for_each(|p| a.open_item(p)),
            CMD_LOCATION => {
                if let Some(p) = item.filter(|p| a.is_managed_path(p)) {
                    crate::win::shell::show_in_folder(&p);
                }
            }
            CMD_RENAME => {
                if let Some(p) = item {
                    a.start_edit(i, EditTarget::Item(p));
                }
            }
            CMD_TO_DESKTOP => {
                if let Some(p) = item {
                    let desktop = a.desktop_dir.clone();
                    a.move_item_to(&p, &desktop);
                }
            }
            CMD_RECYCLE => {
                if let Some(p) = item.filter(|p| a.is_managed_path(p)) {
                    a.defer(Deferred::Recycle(p));
                }
            }
            c if (CMD_SIZE..CMD_SIZE + SIZES.len()).contains(&c) => {
                let size = SIZES[c - CMD_SIZE].0;
                a.update_panel(i, |p| p.icon_size = size);
            }
            c if (CMD_SORT..CMD_SORT + SORTS.len()).contains(&c) => {
                let sort = SORTS[c - CMD_SORT].0;
                a.update_panel(i, |p| p.sort_by = sort);
            }
            CMD_OPEN_FOLDER => {
                if let Some(p) = a.config_of(i) {
                    let dir = PathBuf::from(&p.folder_path);
                    a.defer(Deferred::Open(dir));
                }
            }
            CMD_SETTINGS => a.open_settings(i),
            CMD_NEW => a.new_panel(),
            CMD_RENAME_TITLE => a.start_edit(i, EditTarget::Title(id.to_string())),
            CMD_COLLAPSE => a.toggle_collapsed(i),
            CMD_LOCK => a.set_locked(!a.store.config().locked),
            CMD_APP_SETTINGS => a.open_app_settings(),
            CMD_DELETE => a.defer(Deferred::DeletePanel(id.to_string())),
            c if c >= CMD_TO_PANEL && c - CMD_TO_PANEL < others.len() => {
                let target = &others[c - CMD_TO_PANEL].0;
                let dest = a.store.panel(target).map(|p| PathBuf::from(&p.folder_path));
                if let (Some(p), Some(dest)) = (item, dest) {
                    a.move_item_to(&p, &dest);
                }
            }
            _ => {}
        }
    });
    run_deferred();
}

// ------------------------------------------------------------ drag and drop

pub fn register_drop_target(hwnd: HWND) {
    dragdrop::register(
        hwnd,
        DropHandler { enter: drop_enter, over: drop_over, leave: drop_leave, drop: drop_drop },
    );
}

fn drop_enter(hwnd: HWND, paths: Vec<PathBuf>) -> bool {
    with_app(|a| {
        let Some(i) = a.panel_index(hwnd) else { return false };
        // This panel's own item, dragged to a new spot.
        let own = match (&a.dragging, paths.as_slice()) {
            (Some((id, dragged)), [one]) if id == &a.panels[i].id && dragged == one => {
                one.file_name().map(|n| n.to_string_lossy().into_owned())
            }
            _ => None,
        };
        a.panels[i].drop = Some(DropState { paths, slot: None, own });
        a.paint(i);
        true
    })
    .unwrap_or(false)
}

fn drop_over(hwnd: HWND, x: i32, y: i32, allowed: DROPEFFECT) -> DROPEFFECT {
    with_app(|a| {
        let Some(i) = a.panel_index(hwnd) else { return DROPEFFECT(0) };
        let Some(cfg) = a.config_of(i).cloned() else { return DROPEFFECT(0) };
        let panel = &a.panels[i];
        let (px, py) =
            ((x - panel.rect.left) as f32 / panel.scale, (y - panel.rect.top) as f32 / panel.scale);
        let l = current_layout(&cfg, panel);
        let own = panel.drop.as_ref().is_some_and(|d| d.own.is_some());
        // Other panels' and Explorer's items land where dropped only in
        // custom order; this panel's own icons can always be arranged.
        let slot = l
            .grid
            .filter(|g| g.area.contains(px, py) && (own || cfg.sort_by == SortBy::Custom))
            .map(|g| g.cell_at(px, py, panel.scroll));
        if let Some(d) = a.panels[i].drop.as_mut()
            && d.slot != slot
        {
            d.slot = slot;
            a.paint(i);
        }
        effect_for(!own && a.store.config().add_mode == AddMode::Move, allowed)
    })
    .unwrap_or(DROPEFFECT(0))
}

fn drop_leave(hwnd: HWND) {
    with_app(|a| {
        if let Some(i) = a.panel_index(hwnd) {
            a.panels[i].drop = None;
            a.paint(i);
        }
    });
}

fn drop_drop(hwnd: HWND, _x: i32, _y: i32) {
    with_app(|a| {
        let Some(i) = a.panel_index(hwnd) else { return };
        let Some(d) = a.panels[i].drop.take() else { return };
        let Some(cfg) = a.config_of(i).cloned() else { return };
        let cols = current_layout(&cfg, &a.panels[i]).grid.map_or(1, |g| g.cols);
        match (d.own, d.slot) {
            (Some(name), Some(cell)) => {
                let layout = a.grid_layout(i, cols);
                let arranged = place_at(&layout, &[name], cell, cols);
                a.arrange_items(i, arranged);
            }
            (Some(_), None) => a.paint(i),
            (None, slot) => a.drop_files(i, d.paths, slot.map(|c| (c, cols))),
        }
    });
    run_deferred();
}

// ------------------------------------------------------------ window procs

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
            WM_APP_EDIT_DONE => {
                a.finish_edit(w.0 != 0);
                Some(LRESULT(0))
            }
            WM_TIMER if w.0 == TIMER_SAVE => {
                a.save_now();
                Some(LRESULT(0))
            }
            WM_TIMER => {
                a.host_timer(w.0);
                Some(LRESULT(0))
            }
            WM_DISPLAYCHANGE => {
                a.displays_changed();
                None
            }
            WM_SETTINGCHANGE => {
                a.settings_changed(l);
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

/// The item at a grid cell of panel `i`, if any.
fn item_at(a: &App, i: usize, cell: usize) -> Option<(String, PathBuf)> {
    let cfg = a.config_of(i)?;
    let cols = current_layout(cfg, &a.panels[i]).grid.map_or(1, |g| g.cols);
    item_at_cell(cfg, &a.panels[i].items, cols, cell)
        .map(|it| (it.file_name.clone(), it.path.clone()))
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
                    start_move(a, i, PointerKind::Resize(edge), None);
                    None
                }
                Hit::Chevron | Hit::Gear => Some(Press::Button(hit)),
                Hit::TitleText | Hit::TitleBar => (!locked).then_some(Press::Title(at)),
                Hit::Cell(cell) => match item_at(a, i, cell) {
                    Some((name, _)) => {
                        let k = name.to_lowercase();
                        let panel = &mut a.panels[i];
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
                        a.panels[i].selected.clear();
                        None
                    }
                },
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
            match a.panels[i].press.clone() {
                Some(Press::Title(at)) if moved_past(at, cursor()) => {
                    a.panels[i].press = None;
                    start_move(a, i, PointerKind::Move, Some(at));
                    return Some(LRESULT(0));
                }
                Some(Press::Item(at, name) | Press::Peek(at, name)) if moved_past(at, cursor()) => {
                    a.panels[i].press = None;
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    let path = Path::new(&cfg.folder_path).join(&name);
                    // A drop on the desktop copies the item there; that copy isn't new.
                    a.ignore_on_desktop(&path, 120_000);
                    a.dragging = Some((cfg.id.clone(), path.clone()));
                    a.defer(Deferred::DragOut { panel: cfg.id.clone(), path });
                    return Some(LRESULT(0));
                }
                _ => {}
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
                Some(Press::Button(Hit::Gear)) if hit == Hit::Gear => a.open_settings(i),
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
                Hit::TitleText => a.start_edit(i, EditTarget::Title(cfg.id.clone())),
                Hit::TitleBar => a.toggle_collapsed(i),
                Hit::Cell(cell) => {
                    if let Some((_, path)) = item_at(a, i, cell) {
                        a.open_item(&path);
                    }
                }
                _ => {}
            }
            Some(LRESULT(0))
        }
        WM_RBUTTONUP => {
            let (x, y) = dip(a, i, l);
            let kind = match panel::hit(&cfg, &a.panels[i], locked, x, y) {
                Hit::TitleText | Hit::TitleBar | Hit::Chevron | Hit::Gear => MenuKind::Title,
                Hit::Cell(cell) => match item_at(a, i, cell) {
                    Some((name, path)) => {
                        let k = name.to_lowercase();
                        if !a.panels[i].selected.contains(&k) {
                            a.panels[i].selected = [k].into();
                            a.paint(i);
                        }
                        MenuKind::Item(path)
                    }
                    None => MenuKind::Background,
                },
                Hit::Peek(n) => match a.panels[i].items.get(n) {
                    Some(it) => MenuKind::Item(it.path.clone()),
                    None => MenuKind::Background,
                },
                Hit::Resize(_) | Hit::Body => MenuKind::Background,
            };
            a.defer(Deferred::Menu { panel: cfg.id.clone(), kind });
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
            let lay = current_layout(&cfg, &a.panels[i]);
            if let Some(g) = lay.grid {
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
                Hit::Chevron | Hit::Gear | Hit::Peek(_) => IDC_HAND,
                _ => IDC_ARROW,
            };
            unsafe { SetCursor(LoadCursorW(None, id).ok()) };
            Some(LRESULT(1))
        }
        WM_KEYDOWN => {
            let key = w.0 as u16;
            let panel = &mut a.panels[i];
            let selected: Vec<PathBuf> = panel
                .items
                .iter()
                .filter(|it| panel.is_selected(it))
                .map(|it| it.path.clone())
                .collect();
            if key == VK_RETURN.0 {
                for p in selected {
                    a.open_item(&p);
                }
            } else if key == VK_F2.0 && selected.len() == 1 {
                a.start_edit(i, EditTarget::Item(selected[0].clone()));
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
        WM_TIMER if w.0 == TIMER_ANIM => {
            a.anim_tick(i);
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
        // A rename box is a popup the panel owns, so it asks the panel for its colors.
        WM_CTLCOLOREDIT => {
            let hdc = HDC(w.0 as *mut _);
            unsafe {
                SetTextColor(hdc, COLORREF(0x00ffffff));
                SetBkColor(hdc, EDIT_BG);
            }
            Some(LRESULT(edit_brush().0 as isize))
        }
        // Alt+F4 on a panel doesn't close it; panels go away from their menu.
        WM_CLOSE => Some(LRESULT(0)),
        _ => None,
    }
}

fn start_move(a: &mut App, i: usize, kind: PointerKind, from: Option<POINT>) {
    if a.store.config().locked {
        return;
    }
    // Moving mid-animation would save a window that isn't where the config says yet.
    a.finish_anim(i);
    let Some(cfg) = a.config_of(i).cloned() else { return };
    let snapping = a.store.config().snapping;
    let scale = a.panels[i].scale;
    let pointer = start_pointer(&cfg, &a.store.config().panels, kind, snapping, scale, from);
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
