import type { ReactNode } from 'react'
import type { CollapseDirection, IconSize, PanelPatch, SortBy } from '../../../shared/types'
import { hexToRgba } from '../color'
import { usePanelState } from '../usePanelState'

const PRESETS: { name: string; bg: string; bar: string; text: string }[] = [
  { name: 'Charcoal', bg: '#1e1e1e', bar: '#000000', text: '#ffffff' },
  { name: 'Ocean', bg: '#0b3d91', bar: '#06265c', text: '#ffffff' },
  { name: 'Forest', bg: '#1f5130', bar: '#123520', text: '#ffffff' },
  { name: 'Plum', bg: '#4a1f5c', bar: '#2f1239', text: '#ffffff' },
  { name: 'Ember', bg: '#7a2e12', bar: '#4d1c0a', text: '#ffffff' },
  { name: 'Frost', bg: '#f2f4f7', bar: '#d5dae1', text: '#111111' }
]

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="row">
      <span className="row-label">{label}</span>
      <span className="row-control">{children}</span>
    </label>
  )
}

function Opacity({ value, onChange }: { value: number; onChange(v: number): void }) {
  return (
    <>
      <input
        type="range"
        min={0}
        max={100}
        value={Math.round(value * 100)}
        onChange={(e) => onChange(Number(e.target.value) / 100)}
      />
      <output>{Math.round(value * 100)}%</output>
    </>
  )
}

export function SettingsView() {
  const state = usePanelState()
  if (!state) return null
  const p = state.panel
  const update = (patch: PanelPatch): void => void window.sorter.updatePanel(patch)

  return (
    <div className="settings">
      <div className="preview" aria-label="Preview">
        <div
          className={`preview-fill bar-${p.collapseDirection}`}
          style={{ background: hexToRgba(p.bgColor, p.bgOpacity) }}
        >
          <div
            className="preview-bar"
            style={{
              background: hexToRgba(p.titleBarColor, p.titleBarOpacity),
              color: p.titleTextColor
            }}
          >
            {p.title || 'Untitled'}
          </div>
        </div>
      </div>

      <section>
        <h2>Title</h2>
        <input
          className="text"
          // Uncontrolled: typing round-trips through the main process, which
          // would make a controlled input's caret jump.
          defaultValue={p.title}
          maxLength={200}
          onChange={(e) => update({ title: e.target.value })}
        />
      </section>

      <section>
        <h2>Presets</h2>
        <div className="presets">
          {PRESETS.map((s) => (
            <button
              key={s.name}
              className="preset"
              title={s.name}
              aria-label={`${s.name} preset`}
              style={{ background: s.bg, borderTopColor: s.bar }}
              onClick={() =>
                update({ bgColor: s.bg, titleBarColor: s.bar, titleTextColor: s.text })
              }
            />
          ))}
        </div>
      </section>

      <section>
        <h2>Background</h2>
        <Row label="Color">
          <input
            type="color"
            value={p.bgColor}
            onChange={(e) => update({ bgColor: e.target.value })}
          />
        </Row>
        <Row label="Opacity">
          <Opacity value={p.bgOpacity} onChange={(v) => update({ bgOpacity: v })} />
        </Row>
      </section>

      <section>
        <h2>Title bar</h2>
        <Row label="Color">
          <input
            type="color"
            value={p.titleBarColor}
            onChange={(e) => update({ titleBarColor: e.target.value })}
          />
        </Row>
        <Row label="Opacity">
          <Opacity value={p.titleBarOpacity} onChange={(v) => update({ titleBarOpacity: v })} />
        </Row>
        <Row label="Text color">
          <input
            type="color"
            value={p.titleTextColor}
            onChange={(e) => update({ titleTextColor: e.target.value })}
          />
        </Row>
      </section>

      <section>
        <h2>Icons</h2>
        <Row label="Size">
          <select
            value={p.iconSize}
            onChange={(e) => update({ iconSize: e.target.value as IconSize })}
          >
            <option value="small">Small</option>
            <option value="medium">Medium</option>
            <option value="large">Large</option>
          </select>
        </Row>
        <Row label="Sort by">
          <select value={p.sortBy} onChange={(e) => update({ sortBy: e.target.value as SortBy })}>
            <option value="name">Name</option>
            <option value="type">Type</option>
            <option value="modified">Date modified</option>
          </select>
        </Row>
        <Row label="Collapsed">
          <input
            type="checkbox"
            checked={p.collapsed}
            onChange={() => void window.sorter.toggleCollapsed()}
          />
        </Row>
        <Row label="Collapse toward">
          <select
            value={p.collapseDirection}
            onChange={(e) => update({ collapseDirection: e.target.value as CollapseDirection })}
          >
            <option value="up">Up (title bar on top)</option>
            <option value="down">Down (title bar at bottom)</option>
            <option value="left">Left (title bar on the left)</option>
            <option value="right">Right (title bar on the right)</option>
          </select>
        </Row>
      </section>

      <section>
        <h2>Folder</h2>
        <div className="folder">
          <code title={p.folderPath}>{p.folderPath}</code>
          <button onClick={() => void window.sorter.chooseFolder()}>Change…</button>
        </div>
      </section>

      <section className="danger">
        <button className="delete" onClick={() => void window.sorter.deletePanel()}>
          Delete panel…
        </button>
      </section>
    </div>
  )
}
