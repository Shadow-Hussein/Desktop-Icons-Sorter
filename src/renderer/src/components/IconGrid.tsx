import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import type { IconSize, PanelItem } from '../../../shared/types'
import { ICON_PIXELS } from '../../../shared/types'
import { IconItem } from './IconItem'

interface Props {
  items: PanelItem[]
  iconSize: IconSize
  textColor: string
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

export function IconGrid({ items, iconSize, textColor }: Props) {
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [renaming, setRenaming] = useState<string | null>(null)
  const gridRef = useRef<HTMLDivElement>(null)
  const overlay = useShortcutOverlay(iconSize)

  useEffect(() => window.sorter.onBeginItemRename(setRenaming), [])

  const px = ICON_PIXELS[iconSize]
  const tile = px + 44

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
      style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${tile}px, 1fr))` }}
      onKeyDown={onKeyDown}
      onMouseDown={(e) => {
        if (e.target === gridRef.current) setSelected(new Set())
      }}
      onContextMenu={(e) => {
        if (e.target !== gridRef.current) return
        e.preventDefault()
        window.sorter.showBackgroundMenu()
      }}
    >
      {items.length === 0 && (
        <div className="empty-hint" style={{ color: textColor }}>
          Drag icons here
        </div>
      )}
      {items.map((item) => (
        <IconItem
          key={item.path}
          item={item}
          iconPx={px}
          iconSize={iconSize}
          overlay={item.isShortcut ? overlay : null}
          width={tile}
          textColor={textColor}
          selected={selected.has(item.path)}
          renaming={renaming === item.path}
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
      ))}
    </div>
  )
}
