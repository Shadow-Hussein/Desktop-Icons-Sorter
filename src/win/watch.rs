//! Watching a folder (not its subfolders) with ReadDirectoryChangesW. The
//! main loop waits on each watch's event alongside window messages, so no
//! thread or timer is needed.

use std::path::Path;

use sorter::catcher::Change;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED,
    FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    ReadDirectoryChangesW,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::CreateEventW;

use super::Wide;

/// Room for a burst of changes; on overflow the watch reports "something changed".
const BUFFER_WORDS: usize = 2048;

pub struct DirWatch {
    dir: HANDLE,
    event: HANDLE,
    // Boxed so their addresses stay put while the OS writes to them.
    overlapped: Box<OVERLAPPED>,
    buffer: Box<[u32; BUFFER_WORDS]>,
    pending: bool,
}

/// What a signaled watch reported.
pub enum Report {
    /// The changes, by name (empty after an overflow: list the folder again).
    Changes(Vec<Change>),
    /// The folder is gone or can't be read any more.
    Failed,
}

impl DirWatch {
    pub fn new(path: &Path) -> Option<Self> {
        let p = Wide::os(path.as_os_str());
        unsafe {
            let dir = CreateFileW(
                p.pcwstr(),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )
            .ok()?;
            let Ok(event) = CreateEventW(None, true, false, None) else {
                let _ = CloseHandle(dir);
                return None;
            };
            let mut w = Self {
                dir,
                event,
                overlapped: Box::new(OVERLAPPED { hEvent: event, ..Default::default() }),
                buffer: Box::new([0; BUFFER_WORDS]),
                pending: false,
            };
            if !w.arm() {
                return None;
            }
            Some(w)
        }
    }

    /// The handle the main loop waits on.
    pub fn event(&self) -> HANDLE {
        self.event
    }

    fn arm(&mut self) -> bool {
        *self.overlapped = OVERLAPPED { hEvent: self.event, ..Default::default() };
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME
            | FILE_NOTIFY_CHANGE_DIR_NAME
            | FILE_NOTIFY_CHANGE_LAST_WRITE
            | FILE_NOTIFY_CHANGE_SIZE;
        let ok = unsafe {
            ReadDirectoryChangesW(
                self.dir,
                self.buffer.as_mut_ptr() as *mut _,
                (BUFFER_WORDS * 4) as u32,
                false,
                filter,
                None,
                Some(&mut *self.overlapped),
                None,
            )
        }
        .is_ok();
        self.pending = ok;
        ok
    }

    /// Reads what changed once the event is signaled, and watches again.
    pub fn read(&mut self) -> Report {
        let mut bytes = 0u32;
        self.pending = false;
        if unsafe { GetOverlappedResult(self.dir, &*self.overlapped, &mut bytes, false) }.is_err() {
            return Report::Failed;
        }
        let changes = if bytes == 0 { Vec::new() } else { self.parse(bytes as usize) };
        if !self.arm() {
            return Report::Failed;
        }
        Report::Changes(changes)
    }

    fn parse(&self, len: usize) -> Vec<Change> {
        let base = self.buffer.as_ptr() as *const u8;
        let mut out = Vec::new();
        let mut offset = 0usize;
        while offset + size_of::<FILE_NOTIFY_INFORMATION>() <= len {
            // SAFETY: the OS wrote FILE_NOTIFY_INFORMATION records into the
            // buffer, DWORD-aligned, chained by NextEntryOffset.
            let info = unsafe { &*(base.add(offset) as *const FILE_NOTIFY_INFORMATION) };
            let chars = info.FileNameLength as usize / 2;
            let name = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), chars) };
            let name = String::from_utf16_lossy(name);
            out.push(match info.Action {
                FILE_ACTION_ADDED | FILE_ACTION_RENAMED_NEW_NAME => Change::Appeared(name),
                FILE_ACTION_REMOVED | FILE_ACTION_RENAMED_OLD_NAME => Change::Vanished(name),
                FILE_ACTION_MODIFIED => Change::Modified(name),
                _ => Change::Modified(name),
            });
            if info.NextEntryOffset == 0 {
                break;
            }
            offset += info.NextEntryOffset as usize;
        }
        out
    }
}

impl Drop for DirWatch {
    fn drop(&mut self) {
        unsafe {
            if self.pending {
                let _ = CancelIoEx(self.dir, Some(&*self.overlapped));
                // Wait for the cancel, so the OS is done with the buffer before it's freed.
                let mut bytes = 0;
                let _ = GetOverlappedResult(self.dir, &*self.overlapped, &mut bytes, true);
            }
            let _ = CloseHandle(self.dir);
            let _ = CloseHandle(self.event);
        }
    }
}
