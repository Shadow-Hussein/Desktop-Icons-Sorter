import { useId, type ReactNode } from 'react'

export interface SegmentOption<T extends string> {
  value: T
  label: ReactNode
  /** Tooltip and accessible name, for options whose label is only an icon. */
  title?: string
}

/** A one-click choice between a few options, as segments or picture tiles.
    Native radios underneath, so arrow keys and focus work as usual. */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  variant = 'seg'
}: {
  label: string
  value: T
  options: SegmentOption<T>[]
  onChange(value: T): void
  variant?: 'seg' | 'tiles'
}) {
  const name = useId()
  return (
    <div className={variant} role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <label key={o.value} title={o.title}>
          <input
            type="radio"
            name={name}
            value={o.value}
            checked={o.value === value}
            aria-label={o.title}
            onChange={() => onChange(o.value)}
          />
          <span className="seg-face">{o.label}</span>
        </label>
      ))}
    </div>
  )
}
