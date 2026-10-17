import { useEffect, useRef, useState } from 'react'
import type { IconSize, PanelItem } from '../../../shared/types'

interface Props {
  item: PanelItem
  iconPx: number
  iconSize: IconSize
  /** Shortcut-arrow overlay drawn over the icon, as Explorer does for .lnk/.url. */
  overlay: string | null
  width: number
  textColor: string
  selected: boolean
  renaming: boolean
  onSelect(additive: boolean): void
  /** New name, or null when the rename was cancelled. */
  onRenameDone(name: string | null): void
}

/** Loads the item's shell icon; refetches when the file or icon size changes. */
export function useIcon(path: string, modifiedMs: number, size: IconSize): string | null {
  const [icon, setIcon] = useState<{ key: string; url: string | null } | null>(null)
  const key = `${path}|${modifiedMs}|${size}`
  useEffect(() => {
    let alive = true
    void window.sorter.getIcon(path).then((url) => alive && setIcon({ key, url }))
    return () => {
      alive = false
    }
  }, [path, key])
  return icon?.key === key ? icon.url : null
}

export function IconItem(props: Props) {
  const { item, iconPx, iconSize, overlay, width, textColor, selected, renaming } = props
  const icon = useIcon(item.path, item.modifiedMs, iconSize)
  const done = useRef(false)

  // Re-arm the guard each time a rename starts.
  useEffect(() => {
    if (renaming) done.current = false
  }, [renaming])

  function finish(name: string | null): void {
    if (done.current) return
    done.current = true
    props.onRenameDone(name)
  }

  return (
    <div
      className={`item${selected ? ' selected' : ''}`}
      style={{ width }}
      title={item.fileName}
      draggable={!renaming}
      onMouseDown={(e) => {
        if (e.button === 0 || !selected) props.onSelect(e.ctrlKey)
      }}
      onDoubleClick={() => !renaming && void window.sorter.openItem(item.path)}
      onContextMenu={(e) => {
        e.preventDefault()
        window.sorter.showItemMenu(item.path)
      }}
      onDragStart={(e) => {
        // Hand the drag to the OS as a real file drag so Explorer, the
        // desktop and other panels can all receive it.
        e.preventDefault()
        window.sorter.startDrag(item.path)
      }}
    >
      <div className="item-icon" style={{ width: iconPx, height: iconPx }}>
        {icon ? (
          <img src={icon} width={iconPx} height={iconPx} alt="" draggable={false} />
        ) : (
          <div className="icon-placeholder" />
        )}
        {icon && overlay && (
          <img
            className="item-overlay"
            src={overlay}
            width={iconPx}
            height={iconPx}
            alt=""
            draggable={false}
          />
        )}
      </div>
      {renaming ? (
        <textarea
          className="item-rename"
          autoFocus
          defaultValue={item.displayName}
          rows={2}
          onFocus={(e) => e.target.select()}
          onBlur={(e) => finish(e.target.value.trim() || null)}
          onKeyDown={(e) => {
            e.stopPropagation()
            if (e.key === 'Enter') {
              e.preventDefault()
              finish(e.currentTarget.value.trim() || null)
            }
            if (e.key === 'Escape') finish(null)
          }}
        />
      ) : (
        <div className="item-label" style={{ color: textColor }}>
          {item.displayName}
        </div>
      )}
    </div>
  )
}
