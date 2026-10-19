import { useEffect, useState } from 'react'
import type { AddMode, AppSettings } from '../../../shared/types'
import { Row } from './Row'
import { Segmented, type SegmentOption } from './Segmented'

const ADD_MODES: SegmentOption<AddMode>[] = [
  { value: 'copy', label: 'Copy' },
  { value: 'move', label: 'Move' }
]

/** Settings for the whole app, opened from the tray or a panel's title bar menu. */
export function AppSettingsView() {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  useEffect(() => {
    const off = window.sorter.onAppSettingsChanged(setSettings)
    void window.sorter.getAppSettings().then(setSettings)
    return off
  }, [])
  if (!settings) return null

  const toggle = (key: 'startWithWindows' | 'locked' | 'snapping' | 'stacking') => ({
    type: 'checkbox',
    role: 'switch',
    checked: settings[key],
    onChange: () => void window.sorter.updateAppSettings({ [key]: !settings[key] })
  })

  return (
    <div className="settings">
      <section>
        <h2>Adding items</h2>
        <Row
          label="Items added to a panel"
          hint={
            settings.addMode === 'copy'
              ? 'Items you drop on a panel, or that it catches from the desktop, are copied. The originals stay where they were.'
              : 'Items you drop on a panel, or that it catches from the desktop, move into its folder and leave where they were.'
          }
          group
        >
          <Segmented
            label="Items added to a panel"
            value={settings.addMode}
            options={ADD_MODES}
            onChange={(v) => void window.sorter.updateAppSettings({ addMode: v })}
          />
        </Row>
      </section>

      <section>
        <h2>Moving panels</h2>
        <Row
          label="Snap while moving"
          hint="Edges stick to screen edges and to other panels' edges."
        >
          <input {...toggle('snapping')} />
        </Row>
        <Row label="Lock panels" hint="Panels can't be moved or resized.">
          <input {...toggle('locked')} />
        </Row>
      </section>

      <section>
        <h2>Collapsing</h2>
        <Row
          label="Stack panels"
          hint="When a panel collapses, the panels against its far edge move in to fill the space, and move back when it expands."
        >
          <input {...toggle('stacking')} />
        </Row>
      </section>

      <section>
        <h2>Startup</h2>
        <Row label="Start with Windows">
          <input {...toggle('startWithWindows')} />
        </Row>
      </section>
    </div>
  )
}
