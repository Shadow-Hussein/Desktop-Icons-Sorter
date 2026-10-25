//! OLE drag and drop: each panel is a drop target for files, and items are
//! dragged out as real shell items, so Explorer, the desktop and other
//! panels can all take them.

use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, POINTL};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE,
    IDropSource, IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP, SHDoDragDrop};
use windows::core::{Ref, Result, implement};

/// What a panel's drop target reports to the app.
#[derive(Clone, Copy)]
pub struct DropHandler {
    /// Files entered the panel; returns whether they can be dropped.
    pub enter: fn(HWND, Vec<PathBuf>) -> bool,
    /// The drag is at screen point (x, y); returns the effect to show.
    pub over: fn(HWND, i32, i32, DROPEFFECT) -> DROPEFFECT,
    pub leave: fn(HWND),
    /// The files were dropped at screen point (x, y).
    pub drop: fn(HWND, i32, i32),
}

/// Reads the file paths out of a CF_HDROP data object.
pub fn paths_of(data: &IDataObject) -> Vec<PathBuf> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let mut out = Vec::new();
    unsafe {
        let Ok(mut medium) = data.GetData(&format) else { return out };
        let drop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        for i in 0..count {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(drop, i, Some(&mut buf));
            out.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
        }
        ReleaseStgMedium(&mut medium);
    }
    out
}

#[implement(IDropTarget)]
struct Target {
    hwnd: HWND,
    handler: DropHandler,
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let paths = data.as_ref().map(paths_of).unwrap_or_default();
        let ok = !paths.is_empty() && (self.handler.enter)(self.hwnd, paths);
        unsafe {
            *effect = if ok {
                (self.handler.over)(self.hwnd, pt.x, pt.y, *effect)
            } else {
                DROPEFFECT_NONE
            };
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        unsafe { *effect = (self.handler.over)(self.hwnd, pt.x, pt.y, *effect) };
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        (self.handler.leave)(self.hwnd);
        Ok(())
    }

    fn Drop(
        &self,
        _data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        unsafe { *effect = (self.handler.over)(self.hwnd, pt.x, pt.y, *effect) };
        (self.handler.drop)(self.hwnd, pt.x, pt.y);
        Ok(())
    }
}

pub fn register(hwnd: HWND, handler: DropHandler) {
    let target: IDropTarget = Target { hwnd, handler }.into();
    let _ = unsafe { RegisterDragDrop(hwnd, &target) };
}

pub fn revoke(hwnd: HWND) {
    let _ = unsafe { RevokeDragDrop(hwnd) };
}

/// The effect to show for a drop that copies or moves: move only when the
/// panel moves items and the source allows it. Drags from our own panels
/// allow only copy and link, and asking for move would cancel the drop; the
/// panel moves or copies either way, so this only picks the cursor.
pub fn effect_for(move_items: bool, allowed: DROPEFFECT) -> DROPEFFECT {
    if move_items && allowed.contains(DROPEFFECT_MOVE) {
        DROPEFFECT_MOVE
    } else if allowed.contains(DROPEFFECT_COPY) {
        DROPEFFECT_COPY
    } else {
        allowed & (DROPEFFECT_MOVE | DROPEFFECT_LINK)
    }
}

/// Drags items (which share a folder) out as shell items, with the shell's
/// own drag image. Runs a modal loop until the drop. Copy or link only: a
/// drag never takes an item out of its panel by surprise ("Move to" in its
/// menu does that).
pub fn drag_out(hwnd: HWND, paths: &[PathBuf]) {
    let Some(data) = super::shellmenu::data_object(hwnd, paths) else { return };
    let _ = unsafe {
        SHDoDragDrop(Some(hwnd), &data, None::<&IDropSource>, DROPEFFECT_COPY | DROPEFFECT_LINK)
    };
}
