import type { ReactNode } from 'react'

/** A labelled settings control; `hint` explains it in a line under the label. */
export function Row({
  label,
  hint,
  children
}: {
  label: string
  hint?: string
  children: ReactNode
}) {
  return (
    <label className={hint ? 'row has-hint' : 'row'}>
      <span className="row-label">
        {label}
        {hint && <span className="row-hint">{hint}</span>}
      </span>
      <span className="row-control">{children}</span>
    </label>
  )
}
