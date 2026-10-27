import { ArrowDown, ArrowUp, Cursor, DotsSixVertical, MapPinArea, Path, Robot, Wall } from '@phosphor-icons/react'
import * as Slider from '@radix-ui/react-slider'
import { Reorder } from 'framer-motion'
import { Plus, Trash2 } from 'lucide-react'
import { useId, type ReactNode } from 'react'
import { MUSIC, SOUNDS, type Act, type Archetype, type Behaviour, type Cond, type GameFile, type Rule, type Seek } from '../api'
import {
  ACT_KINDS,
  addRule,
  generatedWalls,
  removeWall,
  setWallEdge,
  takeOverWalls,
  clamp,
  COND_KINDS,
  defaultAct,
  defaultCond,
  describeRule,
  MAX_RULES,
  moveRule,
  removePlace,
  removeRoute,
  removeRule,
  renamePlace,
  renameRoute,
  reorderRules,
  updateRule,
} from '../gameEdit'
import { Empty, LF, NumField, SelectField, Strip, TextField, Toggle } from './Fields'

export type Tool = 'select' | 'place' | 'route' | 'wall' | 'spawn'
export type Sel = { kind: 'place' | 'route' | 'wall'; index: number } | null

export interface BuilderState {
  arch: number
  tool: Tool
  sel: Sel
  /** Points of the route being drawn (not part of the game file until finished). */
  draft: [number, number][]
}
export const initialBuilder: BuilderState = { arch: 0, tool: 'select', sel: null, draft: [] }

type Edit = { game: GameFile; setGame: (g: GameFile) => void }
type Props = Edit & {
  b: BuilderState
  setB: (b: BuilderState) => void
  finishRoute: () => void
  note: string | null
  worldBoxes: [number, number, number, number][]
  worldSize: number
  /** True when the world comes from a world file: its walls are shaped in the World tab, not here. */
  worldFile: boolean
}

const ICON = { size: 16, weight: 'regular' as const }

// ------------------------------------------------------------------- slider

/** A labelled slider with a readout. The thumb is the accessible control; arrow keys step it. */
export function Range({
  label,
  value,
  min,
  max,
  step,
  onChange,
  mark,
  unit = '',
}: {
  label: string
  value: number
  min: number
  max: number
  step: number
  onChange: (v: number) => void
  /** Optional tick for the default value. */
  mark?: number
  unit?: string
}) {
  const id = useId()
  const span = max - min || 1
  const decimals = step < 1 ? (step < 0.1 ? 2 : 1) : 0
  return (
    <div className="min-w-0">
      <div className="flex justify-between gap-2">
        <span className="lbl truncate" id={id}>
          {label}
        </span>
        <span className="lbl text-fg tabular-nums" aria-hidden>
          {value.toFixed(decimals)}
          {unit}
        </span>
      </div>
      <Slider.Root
        className="relative flex h-6 w-full touch-none select-none items-center"
        value={[clamp(value, min, max)]}
        min={min}
        max={max}
        step={step}
        onValueChange={([v]) => v !== undefined && onChange(v)}
      >
        <Slider.Track className="relative h-px grow bg-line">
          <Slider.Range className="absolute h-px bg-accent" />
        </Slider.Track>
        {mark !== undefined && (
          <span
            aria-hidden
            className="absolute top-[7px] h-[10px] w-px bg-fg-3"
            style={{ left: `${clamp(((mark - min) / span) * 100, 0, 100)}%` }}
          />
        )}
        <Slider.Thumb
          aria-labelledby={id}
          aria-valuetext={`${value.toFixed(decimals)}${unit}`}
          className="block h-4 w-2 cursor-grab border border-accent bg-bg active:cursor-grabbing focus-visible:bg-accent"
        />
      </Slider.Root>
    </div>
  )
}

// ----------------------------------------------------------------- rule row

function opts(names: string[], none: string) {
  return names.length ? names.map((n) => ({ value: n, label: n })) : [{ value: '', label: none }]
}

function CondFields({ c, game, set }: { c: Cond; game: GameFile; set: (c: Cond) => void }) {
  const attrs = opts(game.attrs.map((a) => a.name), '(no attributes)')
  switch (c.kind) {
    case 'sees':
      return (
        <>
          <SelectField<Seek>
            label="Who"
            value={c.who}
            options={[
              { value: 'player', label: 'a player' },
              { value: 'bot', label: 'another bot' },
              { value: 'any', label: 'anyone' },
            ]}
            onChange={(who) => set({ ...c, who })}
          />
          <NumField label="Within" value={c.within} onCommit={(n) => set({ ...c, within: clamp(n, 1, game.interest_radius) })} />
        </>
      )
    case 'attr_below':
    case 'attr_above':
      return (
        <>
          <SelectField label="Attribute" value={c.attr} options={attrs} onChange={(attr) => set({ ...c, attr })} />
          <NumField label="Value" value={c.value} onCommit={(value) => set({ ...c, value })} />
        </>
      )
    case 'in_place':
      return (
        <SelectField
          label="Place"
          value={c.place}
          options={opts(game.places.map((p) => p.name), '(no places yet)')}
          onChange={(place) => set({ ...c, place })}
          className="col-span-2"
        />
      )
    case 'chance':
      return <NumField label="Percent per decision" value={c.pct} onCommit={(pct) => set({ ...c, pct: clamp(pct, 0, 100) })} className="col-span-2" />
    case 'sees_kind':
      return (
        <>
          <SelectField label="Bot type" value={c.bot} options={opts(game.archetypes.map((a) => a.name), '(no bot types)')} onChange={(bot) => set({ ...c, bot })} />
          <NumField label="Within" value={c.within} onCommit={(n) => set({ ...c, within: clamp(n, 1, game.interest_radius) })} />
        </>
      )
    case 'during':
      return (
        <div className="col-span-2 flex flex-wrap gap-x-3">
          {(['dawn', 'day', 'dusk', 'night'] as const).map((p) => (
            <Toggle
              key={p}
              label={p}
              checked={c.parts.includes(p)}
              onChange={(on) => {
                const parts = on ? [...c.parts, p] : c.parts.filter((x) => x !== p)
                if (parts.length > 0) set({ ...c, parts })
              }}
            />
          ))}
        </div>
      )
    case 'weather':
      return (
        <SelectField
          label="Weather"
          value={c.names[0] ?? ''}
          options={[{ value: '', label: 'any weather' }, ...(game.weather ?? []).map((w) => ({ value: w.name, label: w.name }))]}
          onChange={(n) => set({ ...c, names: n === '' ? [] : [n] })}
          className="col-span-2"
        />
      )
    default:
      return null
  }
}

function ActFields({ a, game, set }: { a: Act; game: GameFile; set: (a: Act) => void }) {
  switch (a.kind) {
    case 'patrol':
      return (
        <SelectField
          label="Route"
          value={a.route}
          options={opts(game.routes.map((r) => r.name), '(no routes yet)')}
          onChange={(route) => set({ ...a, route })}
          className="col-span-2"
        />
      )
    case 'goto':
      return (
        <SelectField
          label="Place"
          value={a.place}
          options={opts(game.places.map((p) => p.name), '(no places yet)')}
          onChange={(place) => set({ ...a, place })}
          className="col-span-2"
        />
      )
    case 'say':
      return (
        <>
          <TextField label="Text" value={a.text} onCommit={(text) => set({ ...a, text: text.slice(0, 120) })} />
          <Toggle label="Shout" checked={a.shout} onChange={(shout) => set({ ...a, shout })} />
        </>
      )
    case 'chat':
      return (
        <TextField
          label="Lines, separated by |"
          value={a.lines.join(' | ')}
          onCommit={(t) => {
            const lines = t.split('|').map((l) => l.trim().slice(0, 120)).filter((l) => l !== '').slice(0, 16)
            if (lines.length > 0) set({ ...a, lines })
          }}
          className="col-span-2"
        />
      )
    default:
      return null
  }
}

const COND_OPTIONS = COND_KINDS.map((k) => ({ value: k.kind, label: k.label }))
const ACT_OPTIONS = ACT_KINDS.map((k) => ({ value: k.kind, label: k.label }))

function RuleRow({
  n,
  total,
  rule,
  game,
  onChange,
  onMove,
  onRemove,
}: {
  n: number
  total: number
  rule: Rule
  game: GameFile
  onChange: (r: Rule) => void
  onMove: (to: number) => void
  onRemove: () => void
}) {
  const sentence = describeRule(rule)
  return (
    <div className="flex items-stretch gap-2">
      <div className="flex flex-col items-center text-fg-3 pt-1" aria-hidden>
        <DotsSixVertical {...ICON} className="cursor-grab" />
        <span className="lbl">{n + 1}</span>
      </div>
      <div className="min-w-0 flex-1" role="group" aria-label={`Rule ${n + 1}: ${sentence}`}>
        <div className="grid grid-cols-[3.2rem_1fr_1fr] gap-x-2 items-end">
          <span className="lbl h-6 leading-6">When</span>
          <SelectField
            label="Condition"
            value={rule.when.kind}
            options={COND_OPTIONS}
            onChange={(k) => onChange({ ...rule, when: defaultCond(k, game) })}
            className={rule.when.kind === 'always' ? 'col-span-2' : ''}
          />
          <CondFields c={rule.when} game={game} set={(when) => onChange({ ...rule, when })} />
        </div>
        <div className="grid grid-cols-[3.2rem_1fr_1fr] gap-x-2 items-end">
          <span className="lbl h-6 leading-6">Then</span>
          <SelectField
            label="Action"
            value={rule.then.kind}
            options={ACT_OPTIONS}
            onChange={(k) => onChange({ ...rule, then: defaultAct(k, game) })}
            className={['patrol', 'goto', 'say', 'chat'].includes(rule.then.kind) ? '' : 'col-span-2'}
          />
          <ActFields a={rule.then} game={game} set={(then) => onChange({ ...rule, then })} />
        </div>
        {rule.then.kind === 'indoors' && <p className="text-label text-fg-3">Walks to its home and goes inside, out of sight, until a rule above stops matching.</p>}
        {rule.then.kind === 'say' && <p className="text-label text-fg-3">Speaking does not stop movement; the rules below still run.</p>}
      </div>
      <div className="flex flex-col gap-1">
        <button type="button" className="btn px-1" aria-label={`Move rule ${n + 1} up`} disabled={n === 0} onClick={() => onMove(n - 1)}>
          <ArrowUp {...ICON} aria-hidden />
        </button>
        <button type="button" className="btn px-1" aria-label={`Move rule ${n + 1} down`} disabled={n === total - 1} onClick={() => onMove(n + 1)}>
          <ArrowDown {...ICON} aria-hidden />
        </button>
        <button type="button" className="btn px-1" aria-label={`Delete rule ${n + 1}`} onClick={onRemove}>
          <Trash2 size={16} strokeWidth={1.5} aria-hidden />
        </button>
      </div>
    </div>
  )
}

/** Stable identity for rule objects, so a drag animates the same row instead of re-keying by index. */
const ids = new WeakMap<object, number>()
let nextId = 1
const idOf = (r: Rule): number => {
  let v = ids.get(r)
  if (v === undefined) {
    v = nextId++
    ids.set(r, v)
  }
  return v
}

function RuleStack({ game, setGame, arch, a }: Edit & { arch: number; a: Archetype }) {
  const order = a.rules.map((_, i) => i)
  return (
    <section aria-label="Behaviour rules" className="cell py-2">
      <div className="flex items-end justify-between">
        <h2 className="lbl">Behaviour: first matching rule wins</h2>
        <span className="lbl">
          {a.rules.length}/{MAX_RULES}
        </span>
      </div>
      {a.rules.length === 0 && (
        <p className="text-fg-3 py-2">
          No rules, so this bot just does its plain behaviour below. Add a rule to make it react: flee when hurt, chase what it sees, patrol a route.
        </p>
      )}
      <Reorder.Group
        as="ol"
        axis="y"
        values={order}
        onReorder={(next: number[]) => setGame(reorderRules(game, arch, next))}
        className="list-none p-0 m-0"
      >
        {a.rules.map((r, i) => (
          <Reorder.Item
            as="li"
            key={idOf(r)}
            value={i}
            className="py-2 border-t border-line bg-bg first:border-t-0"
            whileDrag={{ backgroundColor: 'var(--color-bg-2)' }}
            transition={{ duration: 0.15, ease: 'easeOut' }}
          >
            <RuleRow
              n={i}
              total={a.rules.length}
              rule={r}
              game={game}
              onChange={(nr) => {
                ids.set(nr, idOf(r))
                setGame(updateRule(game, arch, i, nr))
              }}
              onMove={(to) => setGame(moveRule(game, arch, i, to))}
              onRemove={() => setGame(removeRule(game, arch, i))}
            />
          </Reorder.Item>
        ))}
      </Reorder.Group>
      <div className="flex items-end gap-2 pt-2">
        <button type="button" className="btn" disabled={a.rules.length >= MAX_RULES} onClick={() => setGame(addRule(game, arch))}>
          <Plus size={16} strokeWidth={1.5} aria-hidden />
          Add rule
        </button>
        <SelectField<Behaviour>
          label="If no rule matches"
          value={a.behaviour}
          options={[
            { value: 'wander', label: 'wander' },
            { value: 'idle', label: 'stand still' },
            { value: 'chase', label: 'chase' },
            { value: 'flee', label: 'flee' },
          ]}
          onChange={(behaviour) => setGame({ ...game, archetypes: game.archetypes.map((x, k) => (k === arch ? { ...x, behaviour } : x)) })}
          className="flex-1"
        />
      </div>
    </section>
  )
}

// ---------------------------------------------------------------------- bot

function BotSection({ game, setGame, arch, a }: Edit & { arch: number; a: Archetype }) {
  const patch = (p: Partial<Archetype>) => setGame({ ...game, archetypes: game.archetypes.map((x, k) => (k === arch ? { ...x, ...p } : x)) })
  const setAttr = (name: string, v: number, def: number) => {
    const attrs = { ...a.attrs }
    // An override equal to the default is not an override.
    if (v === def) delete attrs[name]
    else attrs[name] = v
    patch({ attrs })
  }
  return (
    <section aria-label="Bot attributes" className="cell py-2">
      <h2 className="lbl pb-1">Attributes and movement</h2>
      <div className="grid grid-cols-2 gap-x-4">
        <Range label="Speed ×" value={a.speed_mult} min={0} max={4} step={0.05} onChange={(v) => patch({ speed_mult: Math.round(v * 100) / 100 })} mark={1} />
        <Range label="Sight" value={a.sight} min={0} max={game.interest_radius} step={1} onChange={(v) => patch({ sight: v })} mark={40} unit=" u" />
        {game.attrs.map((at) => {
          const span = at.max - at.min
          const step = span <= 20 ? 0.5 : span <= 300 ? 1 : Math.round(span / 200)
          return (
            <Range
              key={at.name}
              label={`Start ${at.name}`}
              value={a.attrs[at.name] ?? at.default}
              min={at.min}
              max={at.max}
              step={step}
              mark={at.default}
              onChange={(v) => setAttr(at.name, v, at.default)}
            />
          )
        })}
      </div>
      <div className="grid grid-cols-3 gap-x-2 pt-1">
        <SelectField
          label="Home"
          value={a.home ?? ''}
          options={[{ value: '', label: 'anywhere' }, ...game.places.map((p) => ({ value: p.name, label: p.name }))]}
          onChange={(v) => patch({ home: v === '' ? null : v })}
        />
        <SelectField
          label="Attacks with"
          value={a.attack ?? ''}
          options={[{ value: '', label: 'nothing' }, ...game.actions.map((x) => ({ value: x.name, label: x.name }))]}
          onChange={(v) => patch({ attack: v === '' ? null : v })}
        />
        <NumField label="Spawn count" value={a.count} onCommit={(n) => patch({ count: Math.max(0, Math.round(n)) })} />
      </div>
      <div className="grid grid-cols-3 gap-x-2 pt-1">
        <TextField
          label="Also lives in"
          value={(a.homes ?? []).join(', ')}
          onCommit={(t) => {
            const homes = t.split(',').map((n) => n.trim()).filter((n) => game.places.some((p) => p.name === n))
            patch({ homes: homes.length && a.home ? homes : undefined })
          }}
          className="col-span-2"
        />
        <Toggle label="Ambushes at night" checked={!!a.ambush} onChange={(on) => patch({ ambush: on || undefined })} />
      </div>
    </section>
  )
}

// -------------------------------------------------------------- places/routes

const TOOLS: readonly { tool: Tool; label: string; icon: ReactNode; hint: string }[] = [
  { tool: 'select', label: 'Select and move', icon: <Cursor {...ICON} aria-hidden />, hint: 'Click a place or route point to select it. Drag to move it. Drag empty ground to pan.' },
  { tool: 'place', label: 'Add place', icon: <MapPinArea {...ICON} aria-hidden />, hint: 'Click the map to drop a place.' },
  { tool: 'route', label: 'Draw route', icon: <Path {...ICON} aria-hidden />, hint: 'Click the map to add points, then finish the route.' },
  { tool: 'wall', label: 'Draw wall', icon: <Wall {...ICON} aria-hidden />, hint: 'Drag a rectangle on the map to place a wall. This switches the world from generated walls to hand-placed ones.' },
  { tool: 'spawn', label: 'Drop test bot', icon: <Robot {...ICON} aria-hidden />, hint: 'Click the map to spawn one of the selected bot type there. Apply your changes first.' },
]

function Toolbar({ b, setB, finishRoute, worldFile }: Pick<Props, 'b' | 'setB' | 'finishRoute' | 'worldFile'>) {
  // With a world file the walls are the file's, so there is no wall tool here.
  const tools = worldFile ? TOOLS.filter((t) => t.tool !== 'wall') : TOOLS
  const cur = tools.find((t) => t.tool === b.tool) ?? tools[0]!
  return (
    <section aria-label="Map tools" className="cell py-2">
      <div role="group" aria-label="Tool" className="flex flex-wrap gap-1">
        {tools.map((t) => (
          <button
            key={t.tool}
            type="button"
            className={`btn ${b.tool === t.tool ? 'border-accent text-fg' : ''}`}
            aria-pressed={b.tool === t.tool}
            onClick={() => setB({ ...b, tool: t.tool, draft: t.tool === 'route' ? b.draft : [] })}
          >
            {t.icon}
            {t.label}
          </button>
        ))}
      </div>
      <p className="text-fg-3 pt-1" aria-live="polite">
        {cur.hint}
      </p>
      {b.tool === 'route' && b.draft.length > 0 && (
        <div className="flex items-center gap-2 pt-1">
          <span className="text-fg-2">{b.draft.length} point(s)</span>
          <button type="button" className="btn btn-primary" disabled={b.draft.length < 2} onClick={finishRoute}>
            Finish route
          </button>
          <button type="button" className="btn" onClick={() => setB({ ...b, draft: [] })}>
            Cancel
          </button>
        </div>
      )}
    </section>
  )
}

function WallsSection({ game, setGame, b, setB, worldBoxes, worldSize }: Edit & Pick<Props, 'b' | 'setB' | 'worldBoxes' | 'worldSize'>) {
  const walls = game.walls
  return (
    <section aria-label="Walls" className="cell py-2">
      <h2 className="lbl pb-1">Walls</h2>
      {!walls ? (
        <>
          <p className="text-fg-3 pb-2">The walls are generated from the world seed. Take them over to move, add or delete them by hand; applying changes them live and nobody is disconnected (anyone left inside a wall is moved out).</p>
          <button type="button" className="btn" onClick={() => setGame(takeOverWalls(game, worldBoxes))}>
            Edit walls by hand
          </button>
        </>
      ) : (
        <>
          <div className="flex items-center gap-2 pb-1">
            <span className="text-fg-2">{walls.length} wall(s), hand placed</span>
            <button type="button" className="btn" disabled={walls.length === 0} onClick={() => setGame({ ...game, walls: [] })}>
              Clear all
            </button>
            <button type="button" className="btn" onClick={() => setGame(generatedWalls(game))}>
              Back to generated
            </button>
          </div>
          {walls.length === 0 && <Empty>An open field. Use “Draw wall” and drag on the map.</Empty>}
          {walls.map((w, i) => {
            const on = b.sel?.kind === 'wall' && b.sel.index === i
            return (
              <Strip key={i}>
                <div className={`flex items-end gap-2 ${on ? 'border-l-2 border-accent pl-2 -ml-2' : ''}`}>
                  <span className="lbl h-6 leading-6 w-8">#{i + 1}</span>
                  <div className="grid grid-cols-4 gap-x-2 flex-1">
                    {(['X0', 'Y0', 'X1', 'Y1'] as const).map((n, k) => (
                      <NumField key={n} label={`Wall ${i + 1} ${n}`} value={w[k] ?? 0} onCommit={(v) => setGame(setWallEdge(game, i, k as 0 | 1 | 2 | 3, v, worldSize))} />
                    ))}
                  </div>
                  <button type="button" className="btn" aria-pressed={on} onClick={() => setB({ ...b, sel: { kind: 'wall', index: i } })}>
                    Show
                  </button>
                  <button type="button" className="btn px-1" aria-label={`Delete wall ${i + 1}`} onClick={() => { setGame(removeWall(game, i)); setB({ ...b, sel: null }) }}>
                    <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                  </button>
                </div>
              </Strip>
            )
          })}
        </>
      )}
    </section>
  )
}

function PlacesSection({ game, setGame, b, setB }: Edit & Pick<Props, 'b' | 'setB'>) {
  const dup = (n: string, i: number) => game.places.some((p, k) => k !== i && p.name === n)
  const patch = (i: number, p: Partial<GameFile['places'][number]>) =>
    setGame({ ...game, places: game.places.map((x, k) => (k === i ? { ...x, ...p } : x)) })
  return (
    <section aria-label="Places" className="cell py-2">
      <h2 className="lbl pb-1">Places</h2>
      {game.places.length === 0 && <Empty>No places. Use “Add place” and click the map. Bots can spawn in one, wander near it, walk to it or react to being inside it. Give one a title and players see it fade in as they walk in; make it lead somewhere and it is a zone line.</Empty>}
      {game.places.map((p, i) => {
        const on = b.sel?.kind === 'place' && b.sel.index === i
        return (
          <Strip key={i}>
            <div className={`flex items-end gap-2 ${on ? 'border-l-2 border-accent pl-2 -ml-2' : ''}`}>
              <TextField label="Place" value={p.name} invalid={dup(p.name, i)} onCommit={(n) => setGame(renamePlace(game, i, n))} className="flex-1" />
              <button type="button" className="btn" aria-pressed={on} onClick={() => setB({ ...b, sel: { kind: 'place', index: i } })}>
                Show
              </button>
              <button type="button" className="btn px-1" aria-label={`Delete place ${p.name}`} onClick={() => setGame(removePlace(game, i))}>
                <Trash2 size={16} strokeWidth={1.5} aria-hidden />
              </button>
            </div>
            <TextField
              label="Shown to players as"
              value={p.title ?? ''}
              invalid={(p.title ?? '').length > 40}
              onCommit={(t) => patch(i, { title: t.trim().slice(0, 40) || undefined })}
              className="pt-1"
            />
            <div className="grid grid-cols-2 gap-x-2 pt-1">
              <SelectField
                label="Sound here"
                value={p.sound ?? ''}
                options={[{ value: '', label: 'the land’s own' }, ...SOUNDS.map((s) => ({ value: s, label: s }))]}
                onChange={(s) => patch(i, { sound: s || undefined })}
              />
              <SelectField
                label="Music here"
                value={p.music ?? ''}
                options={[{ value: '', label: 'as the moment calls' }, ...MUSIC.map((s) => ({ value: s, label: s }))]}
                onChange={(s) => patch(i, { music: s || undefined })}
              />
            </div>
            <div className="grid grid-cols-3 gap-x-2 pt-1">
              <NumField label="X" value={p.x} onCommit={(n) => patch(i, { x: clamp(n, 0, 65536) })} />
              <NumField label="Y" value={p.y} onCommit={(n) => patch(i, { y: clamp(n, 0, 65536) })} />
              <Range label="Radius" value={p.r} min={1} max={150} step={1} onChange={(r) => patch(i, { r })} />
            </div>
            <div className="flex items-end gap-2 pt-1">
              <Toggle label="Safe (no PvP)" checked={!!p.safe} onChange={(on) => patch(i, { safe: on || undefined })} />
              <Toggle
                label="Leads somewhere"
                checked={!!p.exit}
                onChange={(on) => patch(i, { exit: on ? { x: clamp(p.x + p.r + 10, 0, 65536), y: p.y } : undefined })}
              />
              {p.exit && (
                <div className="grid grid-cols-3 gap-x-2 flex-1">
                  <TextField label="To zone (empty: this map)" value={p.exit.zone ?? ''} onCommit={(z) => patch(i, { exit: { ...p.exit!, zone: z.trim().slice(0, 32) || undefined } })} />
                  <NumField label="Arrive at X" value={p.exit.x} onCommit={(n) => patch(i, { exit: { ...p.exit!, x: clamp(n, 0, 65536) } })} />
                  <NumField label="Arrive at Y" value={p.exit.y} onCommit={(n) => patch(i, { exit: { ...p.exit!, y: clamp(n, 0, 65536) } })} />
                </div>
              )}
            </div>
          </Strip>
        )
      })}
    </section>
  )
}

function RoutesSection({ game, setGame, b, setB }: Edit & Pick<Props, 'b' | 'setB'>) {
  const dup = (n: string, i: number) => game.routes.some((p, k) => k !== i && p.name === n)
  const patch = (i: number, p: Partial<GameFile['routes'][number]>) =>
    setGame({ ...game, routes: game.routes.map((x, k) => (k === i ? { ...x, ...p } : x)) })
  return (
    <section aria-label="Routes" className="cell py-2">
      <h2 className="lbl pb-1">Routes</h2>
      {game.routes.length === 0 && <Empty>No routes. Use “Draw route”, click points on the map, then finish. A bot with a patrol rule walks them in order.</Empty>}
      {game.routes.map((r, i) => {
        const on = b.sel?.kind === 'route' && b.sel.index === i
        return (
          <Strip key={i}>
            <div className={`flex items-end gap-2 ${on ? 'border-l-2 border-accent pl-2 -ml-2' : ''}`}>
              <TextField label="Route" value={r.name} invalid={dup(r.name, i)} onCommit={(n) => setGame(renameRoute(game, i, n))} className="flex-1" />
              <Toggle label="Loop" checked={r.loop} onChange={(loop) => patch(i, { loop })} />
              <button type="button" className="btn" aria-pressed={on} onClick={() => setB({ ...b, sel: { kind: 'route', index: i } })}>
                Show
              </button>
              <button type="button" className="btn px-1" aria-label={`Delete route ${r.name}`} onClick={() => setGame(removeRoute(game, i))}>
                <Trash2 size={16} strokeWidth={1.5} aria-hidden />
              </button>
            </div>
            <div className="flex gap-2 pt-1 items-center">
              <span className="text-fg-3">{r.points.length} points</span>
              <button type="button" className="btn" onClick={() => patch(i, { points: [...r.points].reverse() })}>
                Reverse
              </button>
            </div>
            <details className="pt-1">
              <summary className="lbl cursor-pointer">Edit points by number</summary>
              {r.points.map((pt, k) => (
                <div key={k} className="grid grid-cols-[2rem_1fr_1fr_auto] gap-x-2 items-end">
                  <span className="lbl h-6 leading-6">{k + 1}</span>
                  <NumField label={`Route ${r.name} point ${k + 1} X`} value={pt[0]} onCommit={(n) => patch(i, { points: r.points.map((q, j) => (j === k ? [clamp(n, 0, 65536), q[1]] : q)) })} />
                  <NumField label={`Route ${r.name} point ${k + 1} Y`} value={pt[1]} onCommit={(n) => patch(i, { points: r.points.map((q, j) => (j === k ? [q[0], clamp(n, 0, 65536)] : q)) })} />
                  <button
                    type="button"
                    className="btn px-1"
                    aria-label={`Delete point ${k + 1} of ${r.name}`}
                    disabled={r.points.length <= 2}
                    onClick={() => patch(i, { points: r.points.filter((_, j) => j !== k) })}
                  >
                    <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                  </button>
                </div>
              ))}
            </details>
          </Strip>
        )
      })}
    </section>
  )
}

// -------------------------------------------------------------------- panel

export function Builder(p: Props) {
  const { game, setGame, b, setB } = p
  const arch = Math.min(b.arch, game.archetypes.length - 1)
  const a = game.archetypes[arch]
  return (
    <div>
      <Toolbar b={b} setB={setB} finishRoute={p.finishRoute} worldFile={p.worldFile} />
      {p.note && (
        <p role="status" className="py-1 text-fg-2">
          {p.note}
        </p>
      )}
      <section aria-label="Bot type" className="cell py-2">
        <div className="flex items-end gap-2">
          <LF label="Editing bot type" className="flex-1">
            <select className="field cursor-pointer" value={arch} disabled={!a} onChange={(e) => setB({ ...b, arch: Number(e.target.value) })}>
              {game.archetypes.map((x, i) => (
                <option key={i} value={i} className="bg-bg text-fg">
                  {x.name}
                </option>
              ))}
              {game.archetypes.length === 0 && <option value={0}>(none yet, add one in Bots)</option>}
            </select>
          </LF>
          {a && (
            <span className="h-6 w-4 border border-line" style={{ background: a.color }} role="img" aria-label={`Colour ${a.color}`} />
          )}
        </div>
      </section>
      {a ? (
        <>
          <BotSection game={game} setGame={setGame} arch={arch} a={a} />
          <RuleStack game={game} setGame={setGame} arch={arch} a={a} />
        </>
      ) : (
        <Empty>Add a bot type in the Bots tab first.</Empty>
      )}
      <PlacesSection game={game} setGame={setGame} b={b} setB={setB} />
      <RoutesSection game={game} setGame={setGame} b={b} setB={setB} />
      {p.worldFile ? (
        <section aria-label="Walls" className="cell py-2">
          <h2 className="lbl pb-1">Walls</h2>
          <p className="text-fg-3">The walls come from the world file. Add, remove or reshape them in the World tab.</p>
        </section>
      ) : (
        <WallsSection game={game} setGame={setGame} b={b} setB={setB} worldBoxes={p.worldBoxes} worldSize={p.worldSize} />
      )}
    </div>
  )
}
