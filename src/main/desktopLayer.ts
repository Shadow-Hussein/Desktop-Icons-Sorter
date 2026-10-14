import type { BrowserWindow } from 'electron'

/**
 * Keeps panel windows on the desktop layer on Windows.
 *
 * Each panel is made an *owned* window of Progman (the desktop shell window)
 * and pushed to the bottom of the z-order. Owned windows always stay above
 * their owner, so panels sit directly on the desktop, below normal app
 * windows, and they stay visible when "Show desktop" (Win+D) is used. Unlike
 * parenting into the wallpaper WorkerW, panels keep receiving mouse input.
 *
 * On other platforms every function is a no-op, so the app still runs there
 * (useful for development and UI tests).
 */

const GWLP_HWNDPARENT = -8
const HWND_BOTTOM = 1
const SWP_NOSIZE = 0x0001
const SWP_NOMOVE = 0x0002
const SWP_NOACTIVATE = 0x0010

type HandleValue = number | bigint

interface User32 {
  FindWindowW(cls: string | null, name: string | null): HandleValue
  SetWindowLongPtrW(hwnd: HandleValue, index: number, value: HandleValue): HandleValue
  GetWindowLongPtrW(hwnd: HandleValue, index: number): HandleValue
  SetWindowPos(
    hwnd: HandleValue,
    after: HandleValue,
    x: number,
    y: number,
    cx: number,
    cy: number,
    flags: number
  ): boolean
  IsWindow(hwnd: HandleValue): boolean
}

let user32: User32 | null | undefined

function api(): User32 | null {
  if (user32 !== undefined) return user32
  if (process.platform !== 'win32') return (user32 = null)
  try {
    // Loaded lazily so non-Windows builds never touch the native module.
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const koffi = require('koffi') as typeof import('koffi')
    const lib = koffi.load('user32.dll')
    const is64 = process.arch === 'x64' || process.arch === 'arm64'
    // On 32-bit Windows the *Ptr variants are macros for the plain versions.
    const setLong = is64 ? 'SetWindowLongPtrW' : 'SetWindowLongW'
    const getLong = is64 ? 'GetWindowLongPtrW' : 'GetWindowLongW'
    user32 = {
      FindWindowW: lib.func(
        'intptr_t __stdcall FindWindowW(const char16_t *cls, const char16_t *name)'
      ),
      SetWindowLongPtrW: lib.func(
        `intptr_t __stdcall ${setLong}(intptr_t hwnd, int index, intptr_t value)`
      ),
      GetWindowLongPtrW: lib.func(`intptr_t __stdcall ${getLong}(intptr_t hwnd, int index)`),
      SetWindowPos: lib.func(
        'bool __stdcall SetWindowPos(intptr_t hwnd, intptr_t after, int x, int y, int cx, int cy, uint32_t flags)'
      ),
      IsWindow: lib.func('bool __stdcall IsWindow(intptr_t hwnd)')
    }
  } catch (err) {
    console.error('[desktopLayer] could not load user32 via koffi:', err)
    user32 = null
  }
  return user32
}

function hwndOf(win: BrowserWindow): bigint {
  const buf = win.getNativeWindowHandle()
  return buf.length >= 8 ? buf.readBigUInt64LE(0) : BigInt(buf.readUInt32LE(0))
}

function progman(u: User32): HandleValue {
  return u.FindWindowW('Progman', null)
}

/** Make `win` a desktop-level window. Safe to call repeatedly. */
export function attachToDesktop(win: BrowserWindow): boolean {
  const u = api()
  if (!u || win.isDestroyed()) return false
  const owner = progman(u)
  if (!owner) return false
  const hwnd = hwndOf(win)
  if (BigInt(u.GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT)) !== BigInt(owner)) {
    u.SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner)
  }
  sendToBottom(win)
  return true
}

/** Push `win` to the bottom of the z-order (just above the desktop). */
export function sendToBottom(win: BrowserWindow): void {
  const u = api()
  if (!u || win.isDestroyed()) return
  u.SetWindowPos(hwndOf(win), HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)
}

/**
 * True when the window's owner is gone, e.g. after Explorer restarted and
 * Progman was recreated. Callers should re-run `attachToDesktop`.
 */
export function needsReattach(win: BrowserWindow): boolean {
  const u = api()
  if (!u || win.isDestroyed()) return false
  const owner = u.GetWindowLongPtrW(hwndOf(win), GWLP_HWNDPARENT)
  return !owner || !u.IsWindow(owner) || BigInt(owner) !== BigInt(progman(u))
}

export function isDesktopLayerSupported(): boolean {
  return api() !== null
}
