import { useEffect, useRef, useState, type CSSProperties } from 'react'
import type { PanelConfig } from '../../../shared/types'
import { hexToRgba } from '../color'

const DRAG_THRESHOLD = 3

interface Props {
  panel: PanelConfig
  locked: boolean
}

export function TitleBar({ panel, locked }: Props) {
  const [editing, setEditing] = useState(false)
  // null until the user types, so the input starts from the current title.
  const [draft, setDraft] = useState<string | null>(null)
  const pointer = useRef<{ x: number; y: number; moving: boolean } | null>(null)
  // Guards against blur firing after Enter/Escape already ended the edit.
  const editDone = useRef(false)

  useEffect(
    () =>
      window.sorter.onBeginTitleEdit(() => {
        editDone.current = false
        setDraft(null)
        setEditing(true)
      }),
    []
  )

  function startEdit(): void {
    editDone.current = false
    setDraft(null)
    setEditing(true)
  }

  function finish(save: boolean): void {
    if (editDone.current) return
    editDone.current = true
    setEditing(false)
    if (!save) return
    const title = (draft ?? panel.title).trim()
    if (title && title !== panel.title) void window.sorter.updatePanel({ title })
  }

  const isControl = (t: EventTarget): boolean =>
    t instanceof Element && !!t.closest('button, input')
  const fill = hexToRgba(panel.titleBarColor, panel.titleBarOpacity)
  // A minimal bar has no fill, collapsed or not; a collapsed panel still
  // shows its own background. A soft bar paints its color on a faded layer of
  // its own (--bar-fill, see styles.css).
  const bare = panel.titleBarStyle === 'minimal'
  const soft = panel.titleBarStyle === 'soft'

  return (
    <div
      className="titlebar"
      style={
        {
          background: bare || soft ? 'transparent' : fill,
          '--bar-fill': soft ? fill : undefined,
          color: panel.titleTextColor
        } as CSSProperties
      }
      onPointerDown={(e) => {
        if (e.button !== 0 || locked || isControl(e.target)) return
        pointer.current = { x: e.screenX, y: e.screenY, moving: false }
      }}
      onPointerMove={(e) => {
        const p = pointer.current
        if (!p) return
        if (!p.moving) {
          if (Math.hypot(e.screenX - p.x, e.screenY - p.y) < DRAG_THRESHOLD) return
          p.moving = true
          // Capture only once a drag starts; capturing on pointerdown would
          // retarget double-clicks on the title text to the bar.
          e.currentTarget.setPointerCapture(e.pointerId)
          window.sorter.moveStart()
        }
        window.sorter.pointerMove()
      }}
      onPointerUp={(e) => {
        const p = pointer.current
        pointer.current = null
        if (e.currentTarget.hasPointerCapture(e.pointerId)) {
          e.currentTarget.releasePointerCapture(e.pointerId)
        }
        if (p?.moving) window.sorter.pointerEnd()
      }}
      onDoubleClick={(e) => {
        if (isControl(e.target)) return
        if ((e.target as Element).classList.contains('title-text')) startEdit()
        else void window.sorter.toggleCollapsed()
      }}
      onContextMenu={(e) => {
        e.preventDefault()
        window.sorter.showTitleMenu()
      }}
    >
      <button
        className="tb-btn chevron"
        title={panel.collapsed ? 'Expand' : 'Collapse'}
        aria-label={panel.collapsed ? 'Expand panel' : 'Collapse panel'}
        aria-expanded={!panel.collapsed}
        onClick={() => void window.sorter.toggleCollapsed()}
      >
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <path d="M3 6l5 5 5-5" fill="none" stroke="currentColor" strokeWidth="2" />
        </svg>
      </button>

      {editing ? (
        <input
          className="title-input"
          autoFocus
          value={draft ?? panel.title}
          maxLength={200}
          onChange={(e) => setDraft(e.target.value)}
          onFocus={(e) => e.target.select()}
          onBlur={() => finish(true)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') finish(true)
            if (e.key === 'Escape') finish(false)
          }}
        />
      ) : (
        <span className="title-text" title="Double-click to rename">
          {panel.title || 'Untitled'}
        </span>
      )}

      <button
        className="tb-btn gear"
        title="Panel settings"
        aria-label="Panel settings"
        onClick={() => window.sorter.openSettings()}
      >
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
          <circle cx="8" cy="8" r="2.3" fill="none" stroke="currentColor" strokeWidth="1.6" />
          <path
            d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          />
        </svg>
      </button>
    </div>
  )
}
