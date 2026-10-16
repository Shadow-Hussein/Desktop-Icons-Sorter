import { useCallback, useEffect, useRef, useState, type DragEvent } from 'react'
import type { PanelItem, ResizeEdge } from '../../../shared/types'
import { hexToRgba } from '../color'
import { usePanelState } from '../usePanelState'
import { IconGrid } from './IconGrid'
import { TitleBar } from './TitleBar'

const EDGES: ResizeEdge[] = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw']

function ResizeHandle({ edge }: { edge: ResizeEdge }) {
  const active = useRef(false)
  return (
    <div
      className={`resize resize-${edge}`}
      onPointerDown={(e) => {
        if (e.button !== 0) return
        e.preventDefault()
        e.currentTarget.setPointerCapture(e.pointerId)
        active.current = true
        window.sorter.resizeStart(edge)
      }}
      onPointerMove={() => active.current && window.sorter.pointerMove()}
      onPointerUp={(e) => {
        if (!active.current) return
        active.current = false
        e.currentTarget.releasePointerCapture(e.pointerId)
        window.sorter.pointerEnd()
      }}
    />
  )
}

export function Panel() {
  const state = usePanelState()
  const [items, setItems] = useState<PanelItem[]>([])
  const [dropActive, setDropActive] = useState(false)
  const dragDepth = useRef(0)

  useEffect(() => {
    const off = window.sorter.onItemsChanged(setItems)
    void window.sorter.listItems().then(setItems)
    return off
  }, [])

  const hasFiles = (e: DragEvent): boolean => e.dataTransfer.types.includes('Files')

  const onDrop = useCallback((e: DragEvent) => {
    e.preventDefault()
    dragDepth.current = 0
    setDropActive(false)
    const paths = Array.from(e.dataTransfer.files)
      .map((f) => window.sorter.getPathForFile(f))
      .filter(Boolean)
    if (paths.length) void window.sorter.dropFiles(paths)
  }, [])

  if (!state) return null
  const { panel, locked } = state

  return (
    <div
      className={`panel${panel.collapsed ? ' collapsed' : ''}${dropActive ? ' drop-active' : ''}`}
      style={{ background: hexToRgba(panel.bgColor, panel.bgOpacity) }}
      onDragEnter={(e) => {
        if (!hasFiles(e)) return
        dragDepth.current++
        setDropActive(true)
      }}
      onDragLeave={() => {
        dragDepth.current = Math.max(0, dragDepth.current - 1)
        if (dragDepth.current === 0) setDropActive(false)
      }}
      onDragOver={(e) => {
        if (!hasFiles(e)) return
        e.preventDefault()
        // Drags from our own panels (webContents.startDrag) don't allow
        // 'move', and asking for it cancels the drop. The item is moved
        // either way; this only picks the cursor.
        const allowed = e.dataTransfer.effectAllowed
        e.dataTransfer.dropEffect =
          allowed === 'all' || allowed === 'uninitialized' || /move/i.test(allowed)
            ? 'move'
            : 'copy'
      }}
      onDrop={onDrop}
    >
      <TitleBar panel={panel} locked={locked} />
      {!panel.collapsed && (
        <IconGrid items={items} iconSize={panel.iconSize} textColor={panel.titleTextColor} />
      )}
      {!locked &&
        EDGES.filter((e) => !panel.collapsed || e === 'e' || e === 'w').map((e) => (
          <ResizeHandle key={e} edge={e} />
        ))}
    </div>
  )
}
