import type { ReactNode } from 'react'

/** A labelled settings control; `hint` explains it in a line under the label.
    `group` is for controls holding several inputs, such as `Segmented`: a
    wrapping label would send clicks on its text to the first one. `stacked`
    puts the control under the label, full width. */
export function Row({
  label,
  hint,
  group,
  stacked,
  children
}: {
  label: string
  hint?: string
  group?: boolean
  stacked?: boolean
  children: ReactNode
}) {
  const className = ['row', hint && 'has-hint', stacked && 'stacked'].filter(Boolean).join(' ')
  const content = (
    <>
      <span className="row-label">
        {label}
        {hint && <span className="row-hint">{hint}</span>}
      </span>
      <span className="row-control">{children}</span>
    </>
  )
  return group ? (
    <div className={className}>{content}</div>
  ) : (
    <label className={className}>{content}</label>
  )
}
