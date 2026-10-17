import { Footprints, Pause, PersonSimpleRun, Target } from '@phosphor-icons/react'
import { Copy, Plus, Trash2 } from 'lucide-react'
import { useState } from 'react'
import type { ActionFile, Archetype, AttrFile, Behaviour, GameFile } from '../api'
import { removeArch, renameArch } from '../directorEdit'
import {
  addFromTemplate,
  BOT_TEMPLATES,
  exportArchetype,
  importArchetypes,
  newAction,
  newArchetype,
  newAttr,
  removeAction,
  removeAttr,
  renameAction,
  renameAttr,
} from '../gameEdit'
import { Empty, NumField, SelectField, Strip, TextField, Toggle } from './Fields'

type Edit = { game: GameFile; setGame: (g: GameFile) => void }

const ICON = { size: 16, weight: 'regular' as const }
export const BEHAVIOUR_ICON: Record<Behaviour, JSX.Element> = {
  idle: <Pause {...ICON} aria-hidden />,
  wander: <Footprints {...ICON} aria-hidden />,
  chase: <Target {...ICON} aria-hidden />,
  flee: <PersonSimpleRun {...ICON} aria-hidden />,
}

function IconButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button type="button" className="btn px-1" aria-label={label} title={label} onClick={onClick}>
      {children}
    </button>
  )
}

function AddButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button type="button" className="btn mt-2" onClick={onClick}>
      <Plus size={16} strokeWidth={1.5} aria-hidden />
      {label}
    </button>
  )
}

// ------------------------------------------------------------------ settings

export function SettingsEditor({ game, setGame }: Edit) {
  const set = (k: 'move_speed' | 'entity_radius' | 'interest_radius' | 'max_visible') => (n: number) =>
    setGame({ ...game, [k]: n })
  return (
    <section aria-label="World settings" className="cell py-2">
      <h2 className="lbl pb-1">World settings</h2>
      <div className="grid grid-cols-4 gap-x-2">
        <NumField label="Speed u/s" value={game.move_speed} onCommit={set('move_speed')} />
        <NumField label="Radius" value={game.entity_radius} onCommit={set('entity_radius')} />
        <NumField label="Interest" value={game.interest_radius} onCommit={set('interest_radius')} />
        <NumField label="Max seen" value={game.max_visible} onCommit={set('max_visible')} />
      </div>
    </section>
  )
}

// ---------------------------------------------------------------- attributes

export function AttrsEditor({ game, setGame }: Edit) {
  const patch = (i: number, p: Partial<AttrFile>) =>
    setGame({ ...game, attrs: game.attrs.map((a, k) => (k === i ? { ...a, ...p } : a)) })
  const dup = (name: string, i: number) => game.attrs.some((a, k) => k !== i && a.name === name)
  return (
    <div>
      <SettingsEditor game={game} setGame={setGame} />
      {game.attrs.length === 0 && <Empty>No attributes. Health is just one of these; add whatever your game needs.</Empty>}
      {game.attrs.map((a, i) => (
        <Strip key={i}>
          <div className="flex items-end gap-2">
            <TextField
              label={`Attribute ${i}`}
              value={a.name}
              invalid={dup(a.name, i)}
              onCommit={(n) => setGame(renameAttr(game, i, n))}
              className="flex-1"
            />
            <IconButton label={`Delete attribute ${a.name}`} onClick={() => setGame(removeAttr(game, i))}>
              <Trash2 size={16} strokeWidth={1.5} aria-hidden />
            </IconButton>
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-2">
            <NumField label="Min" value={a.min} onCommit={(n) => patch(i, { min: n })} />
            <NumField label="Max" value={a.max} onCommit={(n) => patch(i, { max: n })} />
            <NumField label="Start" value={a.default} onCommit={(n) => patch(i, { default: n })} />
            <NumField label="Regen /s" value={a.regen_per_sec} onCommit={(n) => patch(i, { regen_per_sec: n })} />
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-1">
            <SelectField
              label="Bits"
              value={String(a.bits) as '8' | '16'}
              options={[
                { value: '8', label: '8' },
                { value: '16', label: '16' },
              ]}
              onChange={(v) => patch(i, { bits: v === '16' ? 16 : 8 })}
            />
            <Toggle label="Sent" checked={a.replicated} onChange={(v) => patch(i, { replicated: v })} />
            <SelectField
              label="At min"
              value={a.on_zero}
              options={[
                { value: 'none', label: 'nothing' },
                { value: 'respawn', label: 'respawn' },
              ]}
              onChange={(v) => patch(i, { on_zero: v })}
              className="col-span-2"
            />
          </div>
        </Strip>
      ))}
      <AddButton label="Add attribute" onClick={() => setGame({ ...game, attrs: [...game.attrs, newAttr(game)] })} />
    </div>
  )
}

// ------------------------------------------------------------------- actions

export function ActionsEditor({ game, setGame }: Edit) {
  const attrNames = game.attrs.map((a) => ({ value: a.name, label: a.name }))
  const patch = (i: number, p: Partial<ActionFile>) =>
    setGame({ ...game, actions: game.actions.map((a, k) => (k === i ? { ...a, ...p } : a)) })
  const first = game.attrs[0]?.name ?? ''
  return (
    <div>
      {game.actions.length === 0 && <Empty>No actions. Players and bots can only move.</Empty>}
      {game.actions.map((a, i) => (
        <Strip key={i}>
          <div className="flex items-end gap-2">
            <TextField label="Action" value={a.name} onCommit={(n) => setGame(renameAction(game, i, n))} className="flex-1" />
            <IconButton label={`Delete action ${a.name}`} onClick={() => setGame(removeAction(game, i))}>
              <Trash2 size={16} strokeWidth={1.5} aria-hidden />
            </IconButton>
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-2">
            <NumField label="Id" value={a.id} onCommit={(n) => patch(i, { id: Math.round(n) })} />
            <NumField label="Cooldown ticks" value={a.cooldown_ticks} onCommit={(n) => patch(i, { cooldown_ticks: Math.max(0, Math.round(n)) })} />
            <NumField label="Range" value={a.range} onCommit={(n) => patch(i, { range: n })} />
          </div>
          <div className="pt-1">
            <p className="lbl">Needs</p>
            {a.requires ? (
              <div className="flex items-end gap-2">
                <SelectField
                  label="Own attribute"
                  value={a.requires.attr}
                  options={attrNames}
                  onChange={(v) => patch(i, { requires: { attr: v, min: a.requires?.min ?? 0 } })}
                  className="flex-1"
                />
                <NumField
                  label="At least"
                  value={a.requires.min}
                  onCommit={(n) => patch(i, { requires: { attr: a.requires?.attr ?? first, min: n } })}
                  className="w-24"
                />
                <IconButton label="Remove requirement" onClick={() => patch(i, { requires: null })}>
                  <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                </IconButton>
              </div>
            ) : (
              <button type="button" className="btn" disabled={!first} onClick={() => patch(i, { requires: { attr: first, min: 1 } })}>
                <Plus size={16} strokeWidth={1.5} aria-hidden />
                Add requirement
              </button>
            )}
          </div>
          <div className="pt-1">
            <p className="lbl">Effects</p>
            {a.effects.map((e, k) => (
              <div key={k} className="flex items-end gap-2">
                <SelectField
                  label="On"
                  value={e.to}
                  options={[
                    { value: 'target', label: 'target' },
                    { value: 'self', label: 'self' },
                  ]}
                  onChange={(v) => patch(i, { effects: a.effects.map((x, j) => (j === k ? { ...x, to: v } : x)) })}
                  className="w-24"
                />
                <SelectField
                  label="Attribute"
                  value={e.attr}
                  options={attrNames}
                  onChange={(v) => patch(i, { effects: a.effects.map((x, j) => (j === k ? { ...x, attr: v } : x)) })}
                  className="flex-1"
                />
                <NumField
                  label="Add"
                  value={e.delta}
                  onCommit={(n) => patch(i, { effects: a.effects.map((x, j) => (j === k ? { ...x, delta: n } : x)) })}
                  className="w-24"
                />
                <IconButton label="Remove effect" onClick={() => patch(i, { effects: a.effects.filter((_, j) => j !== k) })}>
                  <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                </IconButton>
              </div>
            ))}
            <button
              type="button"
              className="btn mt-1"
              disabled={!first}
              onClick={() => patch(i, { effects: [...a.effects, { to: 'target', attr: first, delta: -1 }] })}
            >
              <Plus size={16} strokeWidth={1.5} aria-hidden />
              Add effect
            </button>
          </div>
        </Strip>
      ))}
      <AddButton label="Add action" onClick={() => setGame({ ...game, actions: [...game.actions, newAction(game)] })} />
    </div>
  )
}

// ---------------------------------------------------------------------- bots

export function BotsEditor({
  game,
  setGame,
  liveCounts,
  onSpawn,
  onKillAll,
}: Edit & {
  liveCounts: number[]
  onSpawn: (name: string, n: number) => void
  onKillAll: () => void
}) {
  const patch = (i: number, p: Partial<Archetype>) =>
    setGame({ ...game, archetypes: game.archetypes.map((a, k) => (k === i ? { ...a, ...p } : a)) })
  const dup = (name: string, i: number) => game.archetypes.some((a, k) => k !== i && a.name === name)
  return (
    <div>
      {game.archetypes.length === 0 && <Empty>No bot types yet. A bot is an ordinary entity with a behaviour and its own starting attributes.</Empty>}
      {game.archetypes.map((a, i) => {
        const free = game.attrs.filter((x) => !(x.name in a.attrs))
        return (
          <Strip key={i}>
            <div className="flex items-end gap-2">
              <label className="block">
                <span className="lbl block">Colour</span>
                <input
                  type="color"
                  value={a.color}
                  aria-label={`Colour of ${a.name}`}
                  onChange={(e) => patch(i, { color: e.target.value })}
                  className="block h-6 w-8 cursor-pointer border border-line bg-transparent p-0 rounded-sm"
                />
              </label>
              <TextField label="Bot type" value={a.name} invalid={dup(a.name, i)} onCommit={(n) => setGame(renameArch(game, i, n))} className="flex-1" />
              <IconButton
                label={`Delete bot type ${a.name}`}
                onClick={() => setGame(removeArch(game, i))}
              >
                <Trash2 size={16} strokeWidth={1.5} aria-hidden />
              </IconButton>
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-2">
              <div className="min-w-0">
                <SelectField
                  label="Behaviour"
                  value={a.behaviour}
                  options={[
                    { value: 'idle', label: 'idle' },
                    { value: 'wander', label: 'wander' },
                    { value: 'chase', label: 'chase' },
                    { value: 'flee', label: 'flee' },
                  ]}
                  onChange={(v) => patch(i, { behaviour: v })}
                />
              </div>
              <NumField label="Speed ×" value={a.speed_mult} onCommit={(n) => patch(i, { speed_mult: n })} />
              <NumField label="Sight" value={a.sight} onCommit={(n) => patch(i, { sight: n })} />
              <NumField label="Spawn count" value={a.count} onCommit={(n) => patch(i, { count: Math.max(0, Math.round(n)) })} />
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-1">
              <SelectField
                label="Attacks with"
                value={a.attack ?? ''}
                options={[{ value: '', label: 'nothing' }, ...game.actions.map((x) => ({ value: x.name, label: x.name }))]}
                onChange={(v) => patch(i, { attack: v === '' ? null : v })}
                className="col-span-2"
              />
              <div className="col-span-2 flex items-end gap-1 text-fg-2">
                <span className="flex items-center gap-1 h-6" title={a.behaviour}>
                  {BEHAVIOUR_ICON[a.behaviour]}
                </span>
                <span className="h-6 truncate" aria-label={`${liveCounts[i] ?? 0} alive`}>
                  {liveCounts[i] ?? 0} alive
                </span>
              </div>
            </div>
            <div className="pt-1">
              <p className="lbl">Starting attributes that differ</p>
              {Object.entries(a.attrs).map(([k, v]) => (
                <div key={k} className="flex items-end gap-2">
                  <span className="flex-1 h-6 truncate border-b border-line">{k}</span>
                  <NumField
                    label="Value"
                    value={v}
                    onCommit={(n) => patch(i, { attrs: { ...a.attrs, [k]: n } })}
                    className="w-24"
                  />
                  <IconButton
                    label={`Remove override ${k}`}
                    onClick={() => patch(i, { attrs: Object.fromEntries(Object.entries(a.attrs).filter(([x]) => x !== k)) })}
                  >
                    <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                  </IconButton>
                </div>
              ))}
              {free.length > 0 && (
                <label className="block pt-1">
                  <span className="sr-only">Override a starting attribute for {a.name}</span>
                  <select
                    className="field cursor-pointer text-fg-3"
                    value=""
                    onChange={(e) => {
                      const at = game.attrs.find((x) => x.name === e.target.value)
                      if (at) patch(i, { attrs: { ...a.attrs, [at.name]: at.default } })
                    }}
                  >
                    <option value="" className="bg-bg">
                      + override an attribute
                    </option>
                    {free.map((x) => (
                      <option key={x.name} value={x.name} className="bg-bg text-fg">
                        {x.name}
                      </option>
                    ))}
                  </select>
                </label>
              )}
            </div>
            <div className="flex gap-2 pt-2">
              <button type="button" className="btn" onClick={() => onSpawn(a.name, 1)}>
                Spawn 1
              </button>
              <button type="button" className="btn" onClick={() => onSpawn(a.name, 10)}>
                Spawn 10
              </button>
              <button type="button" className="btn" onClick={() => void navigator.clipboard?.writeText(exportArchetype(a))} aria-label={`Copy ${a.name} as JSON`}>
                <Copy size={16} strokeWidth={1.5} aria-hidden />
                Copy JSON
              </button>
            </div>
          </Strip>
        )
      })}
      <div className="flex gap-2 mt-2">
        <AddButton label="Add bot type" onClick={() => setGame({ ...game, archetypes: [...game.archetypes, newArchetype(game)] })} />
        <button type="button" className="btn mt-2" onClick={onKillAll}>
          <Trash2 size={16} strokeWidth={1.5} aria-hidden />
          Remove all live bots
        </button>
      </div>
      <QuickAdd game={game} setGame={setGame} />
    </div>
  )
}

/** The fast ways to get a new bot: a template, pasted JSON, or a file in the server's bots folder. */
function QuickAdd({ game, setGame }: Edit) {
  const [text, setText] = useState('')
  const [note, setNote] = useState<string | null>(null)
  return (
    <div className="pt-4 grid gap-2 max-w-[40rem]">
      <label className="block">
        <span className="lbl">Start from a template</span>
        <select
          className="field cursor-pointer"
          value=""
          onChange={(e) => {
            if (e.target.value) setGame(addFromTemplate(game, e.target.value))
          }}
        >
          <option value="" className="bg-bg">
            choose one…
          </option>
          {BOT_TEMPLATES.map((t) => (
            <option key={t.id} value={t.id} className="bg-bg text-fg">
              {t.label}
            </option>
          ))}
        </select>
      </label>
      <label className="block">
        <span className="lbl">Or paste a bot type as JSON</span>
        <textarea
          className="field !h-24 py-1 font-mono"
          spellCheck={false}
          value={text}
          onChange={(e) => {
            setText(e.target.value)
            setNote(null)
          }}
          placeholder='{"name": "warden", "behaviour": "wander", "count": 4}'
        />
      </label>
      <div className="flex items-center gap-3">
        <button
          type="button"
          className="btn"
          disabled={!text.trim()}
          onClick={() => {
            const r = importArchetypes(game, text)
            if ('error' in r) return setNote(r.error)
            setGame(r.game)
            setText('')
            setNote(`Added ${r.names.join(', ')}. Press Apply to start them.`)
          }}
        >
          Add from JSON
        </button>
        {note && (
          <span role="status" className="text-fg-3">
            {note}
          </span>
        )}
      </div>
      <p className="m-0 text-label text-fg-3">
        No console needed either: drop a <code>.json</code> file in the server's <code>bots.d</code> folder and the bot joins the running world within two
        seconds, with nobody disconnected. “Copy JSON” on any bot above gives you the file's contents.
      </p>
    </div>
  )
}
