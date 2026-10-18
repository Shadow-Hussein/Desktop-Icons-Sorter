import type {
  CollapseAlign,
  CollapseDirection,
  HeaderShape,
  IconSize,
  PanelPatch,
  SortBy
} from '../../../shared/types'
import { hexToRgba } from '../color'
import { usePanelState } from '../usePanelState'
import { Row } from './Row'

const PRESETS: { name: string; bg: string; bar: string; text: string }[] = [
  { name: 'Charcoal', bg: '#1e1e1e', bar: '#000000', text: '#ffffff' },
  { name: 'Ocean', bg: '#0b3d91', bar: '#06265c', text: '#ffffff' },
  { name: 'Forest', bg: '#1f5130', bar: '#123520', text: '#ffffff' },
  { name: 'Plum', bg: '#4a1f5c', bar: '#2f1239', text: '#ffffff' },
  { name: 'Ember', bg: '#7a2e12', bar: '#4d1c0a', text: '#ffffff' },
  { name: 'Frost', bg: '#f2f4f7', bar: '#d5dae1', text: '#111111' }
]

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
  const sideways = p.collapseDirection === 'left' || p.collapseDirection === 'right'

  return (
    <div className="settings">
      <div className="preview" aria-label="Preview">
        <div
          className={`preview-fill bar-${p.collapseDirection} shape-${p.headerShape}`}
          style={{ background: hexToRgba(p.bgColor, p.bgOpacity) }}
        >
          <div
            className="preview-bar"
            style={{
              background:
                p.headerShape === 'minimal'
                  ? 'transparent'
                  : hexToRgba(p.titleBarColor, p.titleBarOpacity),
              color: p.titleTextColor
            }}
          >
            {p.title || 'Untitled'}
          </div>
          {(p.headerShape === 'phone' || p.headerShape === 'folder') && (
            <div className="preview-peek">
              <span />
              <span />
              <span />
              <span />
            </div>
          )}
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
        <h2>Look</h2>
        <Row label="Title bar shape">
          <select
            value={p.headerShape}
            onChange={(e) => update({ headerShape: e.target.value as HeaderShape })}
          >
            <option value="default">Default</option>
            <option value="phone">Dock (row of apps when collapsed)</option>
            <option value="folder">Tile (2 × 2 square of apps when collapsed)</option>
            <option value="pill">Pill</option>
            <option value="minimal">Minimal (no bar until collapsed)</option>
          </select>
        </Row>
        <Row label="Background">
          <input
            type="color"
            aria-label="Background color"
            value={p.bgColor}
            onChange={(e) => update({ bgColor: e.target.value })}
          />
          <Opacity value={p.bgOpacity} onChange={(v) => update({ bgOpacity: v })} />
        </Row>
        <Row label="Title bar">
          <input
            type="color"
            aria-label="Title bar color"
            value={p.titleBarColor}
            onChange={(e) => update({ titleBarColor: e.target.value })}
          />
          <Opacity value={p.titleBarOpacity} onChange={(v) => update({ titleBarOpacity: v })} />
        </Row>
        <Row label="Title text">
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
      </section>

      <section>
        <h2>Collapsing</h2>
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
        {p.headerShape === 'folder' && (
          <Row label="Tile position">
            <select
              value={p.collapseAlign}
              onChange={(e) => update({ collapseAlign: e.target.value as CollapseAlign })}
            >
              {sideways ? (
                <>
                  <option value="start">Top</option>
                  <option value="center">Center</option>
                  <option value="end">Bottom</option>
                </>
              ) : (
                <>
                  <option value="start">Left</option>
                  <option value="center">Center</option>
                  <option value="end">Right</option>
                </>
              )}
            </select>
          </Row>
        )}
        <Row
          label="Stack with other panels"
          hint={
            state.stacking
              ? 'Moves the panels stacked against it as it collapses, and moves with them.'
              : 'Stacking is off for all panels in app settings.'
          }
        >
          <input
            type="checkbox"
            checked={p.stackable}
            disabled={!state.stacking}
            onChange={(e) => update({ stackable: e.target.checked })}
          />
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
