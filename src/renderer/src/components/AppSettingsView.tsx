import { useEffect, useState } from 'react'
import type { AppSettings } from '../../../shared/types'
import { Row } from './Row'

/** Settings for the whole app, opened from the tray or a panel's title bar menu. */
export function AppSettingsView() {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  useEffect(() => {
    const off = window.sorter.onAppSettingsChanged(setSettings)
    void window.sorter.getAppSettings().then(setSettings)
    return off
  }, [])
  if (!settings) return null

  const toggle = (key: keyof AppSettings) => ({
    type: 'checkbox',
    checked: settings[key],
    onChange: () => void window.sorter.updateAppSettings({ [key]: !settings[key] })
  })

  return (
    <div className="settings">
      <section>
        <h2>Moving</h2>
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
