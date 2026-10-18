import type { IconSize, PanelItem } from '../../../shared/types'
import { useIcon } from './IconItem'

const PEEK_ICON = 32

interface Props {
  items: PanelItem[]
  iconSize: IconSize
  textColor: string
  /** How many apps to show at most. */
  max: number
}

function PeekApp({ item, iconSize }: { item: PanelItem; iconSize: IconSize }) {
  // The panel's own icon size, so these reuse the icons the grid already loaded.
  const icon = useIcon(item.path, item.modifiedMs, iconSize)
  return (
    <button
      className="peek-app"
      title={item.displayName}
      aria-label={`Open ${item.displayName}`}
      draggable
      onClick={() => void window.sorter.openItem(item.path)}
      onContextMenu={(e) => {
        e.preventDefault()
        e.stopPropagation()
        window.sorter.showItemMenu(item.path)
      }}
      onDragStart={(e) => {
        e.preventDefault()
        window.sorter.startDrag(item.path)
      }}
    >
      {icon ? (
        <img src={icon} width={PEEK_ICON} height={PEEK_ICON} alt="" draggable={false} />
      ) : (
        <div className="icon-placeholder" />
      )}
    </button>
  )
}

/** The apps a collapsed dock or tile panel shows under its title. */
export function AppPeek({ items, iconSize, textColor, max }: Props) {
  return (
    <div
      className="peek"
      onContextMenu={(e) => {
        e.preventDefault()
        window.sorter.showBackgroundMenu()
      }}
    >
      {items.length === 0 ? (
        <span className="peek-empty" style={{ color: textColor }}>
          Drag icons here
        </span>
      ) : (
        items
          .slice(0, max)
          .map((item) => <PeekApp key={item.path} item={item} iconSize={iconSize} />)
      )}
    </div>
  )
}
