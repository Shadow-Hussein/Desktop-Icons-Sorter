import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent
} from 'react'
import { layoutGrid, placeAt } from '../../../shared/grid'
import type { IconSize, PanelItem, PlacedItem, SortBy } from '../../../shared/types'
import { ICON_PIXELS } from '../../../shared/types'
import { IconItem } from './IconItem'

interface Props {
  items: PanelItem[]
  /** Where the user put items (PanelConfig.cells); used in custom order. */
  cells: PlacedItem[]
  iconSize: IconSize
  textColor: string
  sortBy: SortBy
  /** A drop on the grid was handled here rather than by the panel. */
  onDropHandled(): void
}

/** The shortcut-arrow overlay for this icon size; fetched once per size. */
function useShortcutOverlay(size: IconSize): string | null {
  const [overlay, setOverlay] = useState<{ size: IconSize; url: string | null } | null>(null)
  useEffect(() => {
    let alive = true
    void window.sorter.getShortcutOverlay().then((url) => alive && setOverlay({ size, url }))
    return () => {
      alive = false
    }
  }, [size])
  return overlay?.size === size ? overlay.url : null
}

/** Must match .grid's padding and gap in styles.css. */
const GRID_PAD = 8
const GRID_GAP = 4
const SLOT_KEY = '\0slot'

export function IconGrid({ items, cells, iconSize, textColor, sortBy, onDropHandled }: Props) {
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [renaming, setRenaming] = useState<string | null>(null)
  /** Path of an icon dragged out of this panel, until the drag ends. */
  const [dragging, setDragging] = useState<string | null>(null)
  /** The cell a drop would land in, while a drag that can be placed is over the grid. */
  const [target, setTarget] = useState<number | null>(null)
  /** Spots just arranged here, shown until the main process sends them back. */
  const [pending, setPending] = useState<{ base: PlacedItem[]; cells: PlacedItem[] } | null>(null)
  const [cols, setCols] = useState(1)
  const gridRef = useRef<HTMLDivElement>(null)
  const overlay = useShortcutOverlay(iconSize)

  useEffect(() => window.sorter.onBeginItemRename(setRenaming), [])

  const px = ICON_PIXELS[iconSize]
  const tile = px + 44
  // Rows have a fixed height so the empty ones the user leaves keep their space.
  const rowHeight = px + 46

  // As many columns as fit; a ResizeObserver follows the panel's width.
  useLayoutEffect(() => {
    const grid = gridRef.current
    if (!grid) return
    const measure = (): void => {
      const width = grid.clientWidth - 2 * GRID_PAD
      setCols(Math.max(1, Math.floor((width + GRID_GAP) / (tile + GRID_GAP))))
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(grid)
    return () => observer.disconnect()
  }, [tile])

  // The main process sent the spots back; let go of the copy.
  if (pending && pending.base !== cells) setPending(null)
  // Other sorts pack the icons; custom order keeps the user's spots and gaps.
  const saved = pending?.base === cells ? pending.cells : sortBy === 'custom' ? cells : []
  const layout = layoutGrid(
    items.map((i) => i.fileName),
    saved,
    cols
  )
  const dragged = dragging ? items.find((i) => i.path === dragging) : undefined

  // While a drag is over the grid, show where everything would end up.
  let shown = layout
  if (target !== null) {
    const preview = placeAt(layout, [dragged ? dragged.fileName : SLOT_KEY], target, cols)
    shown = new Map(preview.map((p) => [p.name, p.row * cols + p.col]))
  }
  const slotCell = target === null ? null : shown.get(dragged ? dragged.fileName : SLOT_KEY)!
  const at = (cell: number): { gridColumn: number; gridRow: number } => ({
    gridColumn: (cell % cols) + 1,
    gridRow: Math.floor(cell / cols) + 1
  })

  // Slide icons to their new spots as the target moves. Offsets are relative
  // to the grid, so scrolling doesn't skew them.
  const offsets = useRef(new Map<string, { x: number; y: number }>())
  const wasArranging = useRef(false)
  const layoutKey = [...shown].map(([name, cell]) => `${name}:${cell}`).join('\n') + `|${cols}`
  useLayoutEffect(() => {
    const grid = gridRef.current
    if (!grid) return
    const animate =
      wasArranging.current && !window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const next = new Map<string, { x: number; y: number }>()
    for (const el of grid.querySelectorAll<HTMLElement>('[data-key]')) {
      const key = el.dataset.key!
      const pos = { x: el.offsetLeft, y: el.offsetTop }
      next.set(key, pos)
      const prev = offsets.current.get(key)
      if (animate && prev && (prev.x !== pos.x || prev.y !== pos.y)) {
        el.animate(
          [
            { transform: `translate(${prev.x - pos.x}px, ${prev.y - pos.y}px)` },
            { transform: 'none' }
          ],
          { duration: 180, easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)' }
        )
      }
    }
    offsets.current = next
    wasArranging.current = target !== null
  }, [layoutKey, target])

  /** The cell under the cursor, anywhere in the grid, empty or not. */
  function cellUnder(e: DragEvent): number {
    const grid = gridRef.current!
    const r = grid.getBoundingClientRect()
    const cellWidth = (grid.clientWidth - 2 * GRID_PAD - (cols - 1) * GRID_GAP) / cols
    const x = e.clientX - r.left - GRID_PAD
    const y = e.clientY - r.top - GRID_PAD + grid.scrollTop
    const col = Math.min(cols - 1, Math.max(0, Math.floor(x / (cellWidth + GRID_GAP))))
    const row = Math.max(0, Math.floor(y / (rowHeight + GRID_GAP)))
    return row * cols + col
  }

  function endDrag(): void {
    setDragging(null)
    setTarget(null)
  }

  function onDragOver(e: DragEvent): void {
    if (!e.dataTransfer.types.includes('Files') || !gridRef.current) return
    // Other panels' and Explorer's items land where dropped only in custom
    // order; this panel's own icons can always be arranged.
    if (!dragged && sortBy !== 'custom') return
    const cell = cellUnder(e)
    if (cell !== target) setTarget(cell)
  }

  function onDrop(e: DragEvent): void {
    e.preventDefault()
    e.stopPropagation()
    onDropHandled()
    const cell = target
    endDrag()
    const paths = Array.from(e.dataTransfer.files)
      .map((f) => window.sorter.getPathForFile(f))
      .filter(Boolean)
    if (
      dragged &&
      cell !== null &&
      paths.length === 1 &&
      paths[0].toLowerCase() === dragged.path.toLowerCase()
    ) {
      const arranged = placeAt(layout, [dragged.fileName], cell, cols)
      setPending({ base: cells, cells: arranged })
      void window.sorter.arrangeItems(arranged)
      return
    }
    if (paths.length) {
      void window.sorter.dropFiles(paths, cell === null ? null : { cell, cols })
    }
  }

  function onKeyDown(e: KeyboardEvent): void {
    if (renaming) return
    const sel = items.filter((i) => selected.has(i.path))
    if (e.key === 'Enter') sel.forEach((i) => void window.sorter.openItem(i.path))
    else if (e.key === 'F2' && sel.length === 1) setRenaming(sel[0].path)
    else if (e.key === 'Escape') setSelected(new Set())
    else if (e.key === 'a' && e.ctrlKey) {
      e.preventDefault()
      setSelected(new Set(items.map((i) => i.path)))
    }
  }

  return (
    <div
      ref={gridRef}
      className="grid"
      tabIndex={0}
      style={{
        gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
        gridAutoRows: rowHeight
      }}
      onKeyDown={onKeyDown}
      onMouseDown={(e) => {
        if (e.target === gridRef.current) setSelected(new Set())
      }}
      // The OS runs the drag, so there's no dragend here: the first plain
      // mouse move after it tells us it's over (dropped elsewhere or cancelled).
      onMouseMove={(e) => {
        if ((dragging || target !== null) && e.buttons === 0) endDrag()
      }}
      onDragOver={onDragOver}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setTarget(null)
      }}
      onDrop={onDrop}
      onContextMenu={(e) => {
        if (e.target !== gridRef.current) return
        e.preventDefault()
        window.sorter.showBackgroundMenu()
      }}
    >
      {items.length === 0 && target === null && (
        <div className="empty-hint" style={{ color: textColor }}>
          Drag icons here
        </div>
      )}
      {slotCell !== null && (
        <div
          key={SLOT_KEY}
          className="item drop-slot"
          data-key={SLOT_KEY}
          style={{ width: tile, ...at(slotCell) }}
        >
          <div className="drop-slot-icon" style={{ width: px, height: px }} />
        </div>
      )}
      {items.map((item) =>
        // While arranging, the dragged icon's spot is the slot.
        item === dragged && target !== null ? null : (
          <IconItem
            key={item.path}
            item={item}
            iconPx={px}
            iconSize={iconSize}
            overlay={item.isShortcut ? overlay : null}
            width={tile}
            cell={at(shown.get(item.fileName) ?? 0)}
            textColor={textColor}
            selected={selected.has(item.path)}
            renaming={renaming === item.path}
            onDragStart={() => setDragging(item.path)}
            onSelect={(additive) =>
              setSelected((prev) => {
                if (!additive) return new Set([item.path])
                const next = new Set(prev)
                if (next.has(item.path)) next.delete(item.path)
                else next.add(item.path)
                return next
              })
            }
            onRenameDone={(name) => {
              setRenaming(null)
              if (name !== null && name !== item.displayName) {
                void window.sorter.renameItem(item.path, name)
              }
              gridRef.current?.focus()
            }}
          />
        )
      )}
    </div>
  )
}
