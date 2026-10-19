import type { CSSProperties } from 'react'
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
import { Segmented, type SegmentOption } from './Segmented'

const PRESETS: { name: string; bg: string; bar: string; text: string }[] = [
  { name: 'Charcoal', bg: '#1e1e1e', bar: '#000000', text: '#ffffff' },
  { name: 'Ocean', bg: '#0b3d91', bar: '#06265c', text: '#ffffff' },
  { name: 'Forest', bg: '#1f5130', bar: '#123520', text: '#ffffff' },
  { name: 'Plum', bg: '#4a1f5c', bar: '#2f1239', text: '#ffffff' },
  { name: 'Ember', bg: '#7a2e12', bar: '#4d1c0a', text: '#ffffff' },
  { name: 'Frost', bg: '#f2f4f7', bar: '#d5dae1', text: '#111111' }
]

const SHAPES: { value: HeaderShape; name: string; hint: string }[] = [
  { value: 'default', name: 'Default', hint: 'A title bar along the edge.' },
  { value: 'phone', name: 'Dock', hint: 'Collapses to a card with a row of its apps.' },
  { value: 'folder', name: 'Tile', hint: 'Collapses to a square with a 2 × 2 grid of its apps.' },
  { value: 'pill', name: 'Pill', hint: 'A rounded capsule for a title bar.' },
  { value: 'minimal', name: 'Minimal', hint: 'No title bar until the panel collapses.' }
]

const SHAPE_OPTIONS: SegmentOption<HeaderShape>[] = SHAPES.map((s) => ({
  value: s.value,
  label: (
    <>
      <span className={`art shape-${s.value}`}>
        <span className="art-bar" />
        {(s.value === 'phone' || s.value === 'folder') && (
          <span className="art-apps">
            <i />
            <i />
            {s.value === 'folder' ? (
              <>
                <i />
                <i />
              </>
            ) : (
              <i />
            )}
          </span>
        )}
      </span>
      {s.name}
    </>
  )
}))

function Arrow({ turn }: { turn: number }) {
  return (
    <svg
      viewBox="0 0 16 16"
      width="14"
      height="14"
      aria-hidden="true"
      style={{ rotate: `${turn}deg` }}
    >
      <path
        d="M8 13V3M4 7l4-4 4 4"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

const DIRECTIONS: SegmentOption<CollapseDirection>[] = [
  { value: 'up', label: <Arrow turn={0} />, title: 'Up (title bar on top)' },
  { value: 'down', label: <Arrow turn={180} />, title: 'Down (title bar at bottom)' },
  { value: 'left', label: <Arrow turn={-90} />, title: 'Left (title bar on the left)' },
  { value: 'right', label: <Arrow turn={90} />, title: 'Right (title bar on the right)' }
]

const ICON_SIZES: SegmentOption<IconSize>[] = [
  { value: 'small', label: 'Small' },
  { value: 'medium', label: 'Medium' },
  { value: 'large', label: 'Large' }
]

function Opacity({ value, onChange }: { value: number; onChange(v: number): void }) {
  return (
    <>
      <input
        type="range"
        min={0}
        max={100}
        value={Math.round(value * 100)}
        style={{ '--f': value } as CSSProperties}
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
  const shape = SHAPES.find((s) => s.value === p.headerShape) ?? SHAPES[0]
  const isPreset = (s: (typeof PRESETS)[number]): boolean =>
    p.bgColor.toLowerCase() === s.bg &&
    p.titleBarColor.toLowerCase() === s.bar &&
    p.titleTextColor.toLowerCase() === s.text

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
          {PRESETS.map((s) => {
            const active = isPreset(s)
            return (
              <button
                key={s.name}
                className={active ? 'preset active' : 'preset'}
                title={s.name}
                aria-label={`${s.name} preset`}
                aria-pressed={active}
                style={{ '--bg': s.bg, '--bar': s.bar } as CSSProperties}
                onClick={() =>
                  update({ bgColor: s.bg, titleBarColor: s.bar, titleTextColor: s.text })
                }
              >
                {active && (
                  <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
                    <path
                      d="M3.5 8.5l3 3 6-7"
                      fill="none"
                      stroke={s.text}
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                )}
              </button>
            )
          })}
        </div>
      </section>

      <section>
        <h2>Look</h2>
        <Row label="Title bar shape" hint={shape.hint} group stacked>
          <Segmented
            label="Title bar shape"
            variant="tiles"
            value={p.headerShape}
            options={SHAPE_OPTIONS}
            onChange={(v) => update({ headerShape: v })}
          />
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
            aria-label="Title text color"
            value={p.titleTextColor}
            onChange={(e) => update({ titleTextColor: e.target.value })}
          />
        </Row>
      </section>

      <section>
        <h2>Icons</h2>
        <Row label="Size" group>
          <Segmented
            label="Icon size"
            value={p.iconSize}
            options={ICON_SIZES}
            onChange={(v) => update({ iconSize: v })}
          />
        </Row>
        <Row
          label="Sort by"
          hint={
            p.sortBy === 'custom'
              ? 'Drag icons to arrange them. New ones go at the end.'
              : 'Dragging an icon to a new spot switches to custom order.'
          }
        >
          <select value={p.sortBy} onChange={(e) => update({ sortBy: e.target.value as SortBy })}>
            <option value="custom">Custom</option>
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
            role="switch"
            checked={p.collapsed}
            onChange={() => void window.sorter.toggleCollapsed()}
          />
        </Row>
        <Row label="Collapse toward" group>
          <Segmented
            label="Collapse toward"
            value={p.collapseDirection}
            options={DIRECTIONS}
            onChange={(v) => update({ collapseDirection: v })}
          />
        </Row>
        {p.headerShape === 'folder' && (
          <Row label="Tile position" group>
            <Segmented<CollapseAlign>
              label="Tile position"
              value={p.collapseAlign}
              options={[
                { value: 'start', label: sideways ? 'Top' : 'Left' },
                { value: 'center', label: 'Center' },
                { value: 'end', label: sideways ? 'Bottom' : 'Right' }
              ]}
              onChange={(v) => update({ collapseAlign: v })}
            />
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
            role="switch"
            checked={p.stackable}
            disabled={!state.stacking}
            onChange={(e) => update({ stackable: e.target.checked })}
          />
        </Row>
      </section>

      <section>
        <h2>New items</h2>
        <Row
          label="Catch new desktop items"
          hint={`Shortcuts, downloads and files that appear on your desktop are ${
            state.addMode === 'copy' ? 'copied' : 'moved'
          } into this panel (change that in app settings). Only one panel catches them.`}
        >
          <input
            type="checkbox"
            role="switch"
            checked={p.catchNewItems}
            onChange={(e) => update({ catchNewItems: e.target.checked })}
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
