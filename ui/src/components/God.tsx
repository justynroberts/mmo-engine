import { CloudFog, CloudLightning, Crosshair, Lightning, Moon, Stop, Sun, SunHorizon } from '@phosphor-icons/react'
import { ArrowDown, ArrowUp, Plus, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { api, type DayPart, type DirectorState, type EventDef, type GameFile, type RunRow, type Step, type WeatherDef, type WeatherZone, type When } from '../api'
import { DEFAULT_DAYLIGHT, describe, moment, nightShare, phase, problem, type Daylight } from '../daylight'
import {
  addEvent,
  addWeather,
  addZone,
  removeZone,
  renameZone,
  defaultStep,
  directorProblems,
  EVENT_PRESETS,
  MAX_EVENTS,
  MAX_STEPS,
  MAX_WEATHER,
  MAX_ZONES,
  removeEvent,
  removeWeather,
  renameEvent,
  renameWeather,
  STEP_KINDS,
  WEATHER_PRESETS,
} from '../directorEdit'
import { clamp } from '../gameEdit'
import { Empty, NumField, SelectField, Strip, TextField, Toggle } from './Fields'

/** A pending "click the map to drop bots" request, owned by the app so the map can act on it. */
export interface Drop {
  arch: string
  count: number
}

interface Props {
  /** The draft being edited. */
  game: GameFile
  /** What the server is running now: spawning and firing only work on what has been applied. */
  live: GameFile | null
  setGame: (g: GameFile) => void
  director: DirectorState | undefined
  drop: Drop | null
  setDrop: (d: Drop | null) => void
  note: string | null
  /** The server moved the day (a "now" button): take its schedule as both what runs and what is being edited. */
  onDaylight: (d: Daylight) => void
  /** Which page this is: spawning and scattering (Bots), day, weather and zones (Sky), or events. */
  part: 'bots' | 'sky' | 'events'
}

const ICON = { size: 16, weight: 'regular' as const }
const iconFor = (w: WeatherDef) =>
  w.drain.length > 0 ? <CloudLightning {...ICON} aria-hidden /> : w.sight_mult < 1 ? <CloudFog {...ICON} aria-hidden /> : <Sun {...ICON} aria-hidden />

function IconButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: React.ReactNode }) {
  return (
    <button type="button" className="btn !px-1" aria-label={label} title={label} disabled={disabled} onClick={onClick}>
      {children}
    </button>
  )
}

const opts = (names: string[]) => names.map((n) => ({ value: n, label: n }))
const withNone = (names: string[], none: string) => [{ value: '', label: none }, ...opts(names)]

export default function God({ game, live, setGame, director, drop, setDrop, note, onDaylight, part }: Props) {
  const [msg, setMsg] = useState<string | null>(null)
  const [bot, setBot] = useState('')
  const [count, setCount] = useState(10)
  const [where, setWhere] = useState('') // '' anywhere, '@map' click the map, else a place name
  const [secs, setSecs] = useState(120)
  const [item, setItem] = useState('')
  const [itemCount, setItemCount] = useState(10)
  const [itemWhere, setItemWhere] = useState('')

  const liveBots = live?.archetypes.map((a) => a.name) ?? []
  const livePlaces = live?.places ?? []
  const liveWeather = new Set((live?.weather ?? []).map((w) => w.name))
  const liveEvents = new Set((live?.events ?? []).map((e) => e.name))
  const botName = liveBots.includes(bot) ? bot : (liveBots[0] ?? '')
  const liveItems = (live?.items ?? []).map((i) => i.name)
  const itemName = liveItems.includes(item) ? item : (liveItems[0] ?? '')
  const problems = directorProblems(game)
  const weather = game.weather ?? []
  const events = game.events ?? []
  const placeNames = game.places.map((p) => p.name)
  const botNames = game.archetypes.map((a) => a.name)
  const attrNames = game.attrs.map((a) => a.name)

  const post = async (path: string, body: unknown, ok: string) => {
    const r = await api<{ error?: string; note?: string }>(path, 'POST', body)
    setMsg(r.status === 200 ? (r.body.note ?? ok) : (r.body.error ?? `the server said ${r.status}`))
  }

  const spawn = () => {
    if (!botName) return
    if (where === '@map') return setDrop({ arch: botName, count })
    setDrop(null)
    const p = livePlaces.find((x) => x.name === where)
    void post('/api/spawn', { archetype: botName, count, ...(p ? { x: p.x, y: p.y, radius: p.r } : {}) }, `Spawned ${count} ${botName}${p ? ` at ${p.name}` : ''}.`)
  }

  const setEvent = (i: number, e: EventDef) => setGame({ ...game, events: events.map((x, k) => (k === i ? e : x)) })
  const setWeather = (i: number, w: WeatherDef) => setGame({ ...game, weather: weather.map((x, k) => (k === i ? w : x)) })

  return (
    <div className="grid gap-1">
      {(msg || note) && (
        <p role="status" className="m-0 py-1 text-fg-2">
          {msg ?? note}
        </p>
      )}
      {problems.length > 0 && (
        <div role="alert" className="text-accent py-1">
          {problems.map((p) => (
            <p key={p} className="m-0">
              {p}
            </p>
          ))}
        </div>
      )}

      {part === 'bots' && (<>
      <section aria-label="Spawn bots" className="cell py-2">
        <h2 className="lbl pb-1">Spawn</h2>
        {liveBots.length === 0 ? (
          <Empty>No bot types are running yet. Add one in Bots and press Apply.</Empty>
        ) : (
          <>
            <div className="grid grid-cols-[1fr_5rem_1fr] gap-2 items-end">
              <SelectField label="Bot type" value={botName} options={opts(liveBots)} onChange={setBot} />
              <NumField label="How many" value={count} onCommit={(n) => setCount(clamp(Math.round(n), 1, 500))} />
              <SelectField label="Where" value={where} options={[{ value: '', label: 'anywhere' }, ...livePlaces.map((p) => ({ value: p.name, label: p.name })), { value: '@map', label: 'click the map' }]} onChange={setWhere} />
            </div>
            <div className="flex items-center gap-2 pt-2">
              <button type="button" className="btn btn-primary" onClick={spawn}>
                {where === '@map' ? <Crosshair {...ICON} aria-hidden /> : <Plus size={16} strokeWidth={1.5} aria-hidden />}
                {where === '@map' ? 'Arm the map' : 'Spawn'}
              </button>
              {drop && (
                <>
                  <span role="status" className="text-fg-2">
                    Click the map to drop {drop.count} {drop.arch}.
                  </span>
                  <button type="button" className="btn" onClick={() => setDrop(null)}>
                    Stop
                  </button>
                </>
              )}
            </div>
          </>
        )}
      </section>

      <section aria-label="Scatter items" className="cell py-2">
        <h2 className="lbl pb-1">Items on the ground</h2>
        {liveItems.length === 0 ? (
          <Empty>No items are running yet. Add one in Items and press Apply.</Empty>
        ) : (
          <>
            <div className="grid grid-cols-[1fr_5rem_1fr] gap-2 items-end">
              <SelectField label="Item" value={itemName} options={opts(liveItems)} onChange={setItem} />
              <NumField label="How many" value={itemCount} onCommit={(n) => setItemCount(clamp(Math.round(n), 1, 500))} />
              <SelectField label="Where" value={itemWhere} options={withNone(livePlaces.map((p) => p.name), 'anywhere')} onChange={setItemWhere} />
            </div>
            <div className="flex items-center gap-2 pt-2">
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => void post('/api/ground', { item: itemName, count: itemCount, ...(itemWhere ? { place: itemWhere } : {}) }, `Scattered ${itemCount} ${itemName}${itemWhere ? ` at ${itemWhere}` : ''}.`)}
              >
                <Plus size={16} strokeWidth={1.5} aria-hidden />
                Scatter
              </button>
              <span className="text-fg-3">They fade after five minutes.</span>
            </div>
          </>
        )}
      </section>
      </>)}

      {part === 'sky' && (<>
      <DaylightSection game={game} live={live} setGame={setGame} setMsg={setMsg} onDaylight={onDaylight} />

      <ZonesSection game={game} live={live} setGame={setGame} director={director} post={post} />

      <section aria-label="Weather" className="cell py-2">
        <h2 className="lbl pb-1">Weather</h2>
        <p className="m-0" role="status">
          {director?.weather ? (
            <>
              Now: <strong>{director.weather.name}</strong>
              {director.weather.left_secs !== null ? `, ${director.weather.left_secs}s left` : ', until cleared'}
            </>
          ) : (
            'Clear sky.'
          )}
        </p>
        <div className="flex flex-wrap items-end gap-2 pt-2">
          <NumField label="Lasts (s, 0 = no end)" value={secs} onCommit={(n) => setSecs(clamp(Math.round(n), 0, 86400))} className="w-56" />
          {weather
            .filter((w) => liveWeather.has(w.name))
            .map((w) => (
              <button key={w.name} type="button" className="btn" onClick={() => void post('/api/weather', { name: w.name, duration_secs: secs }, `${w.name} started.`)}>
                {iconFor(w)}
                {w.name}
              </button>
            ))}
          <button type="button" className="btn" disabled={!director?.weather} onClick={() => void post('/api/weather', { name: null }, 'Sky cleared.')}>
            <Sun {...ICON} aria-hidden />
            Clear sky
          </button>
        </div>
        {weather.some((w) => !liveWeather.has(w.name)) && <p className="m-0 pt-1 text-label text-fg-3">New weather appears as a button once you press Apply.</p>}
      </section>

      <section aria-label="Weather types">
        <h2 className="lbl pt-2">Weather types</h2>
        {weather.length === 0 && <Empty>None yet. Weather changes how far bots see, can slowly hurt or heal everyone, and tells players what to draw.</Empty>}
        {weather.map((w, i) => (
          <Strip key={i}>
            <div className="flex items-end gap-2">
              <TextField label="Weather" value={w.name} onCommit={(n) => setGame(renameWeather(game, i, n))} className="flex-1" invalid={weather.some((x, k) => k !== i && x.name === w.name)} />
              <label className="block">
                <span className="lbl block">Colour</span>
                <input type="color" aria-label={`Colour of ${w.name}`} className="h-6 w-10 bg-transparent border border-line p-0" value={/^#[0-9a-f]{6}$/i.test(w.color) ? w.color : '#8899aa'} onChange={(e) => setWeather(i, { ...w, color: e.target.value })} />
              </label>
              <IconButton label={`Remove ${w.name}`} onClick={() => setGame(removeWeather(game, i))}>
                <Trash2 size={16} strokeWidth={1.5} aria-hidden />
              </IconButton>
            </div>
            <div className="grid grid-cols-2 gap-2 pt-1">
              <NumField label="Thickness (0 to 1)" value={w.intensity} step={0.1} onCommit={(n) => setWeather(i, { ...w, intensity: clamp(n, 0, 1) })} />
              <NumField label="Bot sight (x)" value={w.sight_mult} step={0.1} onCommit={(n) => setWeather(i, { ...w, sight_mult: clamp(n, 0.1, 4) })} />
            </div>
            {w.drain.map((d, k) => (
              <div key={k} className="flex items-end gap-2 pt-1">
                <SelectField label="Each second, change" value={d.attr} options={opts(attrNames)} onChange={(attr) => setWeather(i, { ...w, drain: w.drain.map((x, j) => (j === k ? { ...x, attr } : x)) })} className="flex-1" />
                <NumField label="By" value={d.per_sec} onCommit={(n) => setWeather(i, { ...w, drain: w.drain.map((x, j) => (j === k ? { ...x, per_sec: clamp(n, -1000, 1000) } : x)) })} className="w-20" />
                <IconButton label={`Remove that effect from ${w.name}`} onClick={() => setWeather(i, { ...w, drain: w.drain.filter((_, j) => j !== k) })}>
                  <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                </IconButton>
              </div>
            ))}
            <button type="button" className="btn mt-2" disabled={w.drain.length >= 4 || attrNames.length === 0} onClick={() => setWeather(i, { ...w, drain: [...w.drain, { attr: attrNames[0]!, per_sec: -1 }] })}>
              <Plus size={16} strokeWidth={1.5} aria-hidden />
              Add effect
            </button>
          </Strip>
        ))}
        <div className="flex gap-2 pt-2">
          <button type="button" className="btn" disabled={weather.length >= MAX_WEATHER} onClick={() => setGame(addWeather(game))}>
            <Plus size={16} strokeWidth={1.5} aria-hidden />
            Add weather
          </button>
          <label className="block">
            <span className="sr-only">Add a weather from a preset</span>
            <select className="field cursor-pointer text-fg-3" value="" onChange={(e) => e.target.value && setGame(addWeather(game, e.target.value))}>
              <option value="" className="bg-bg">
                or from a preset…
              </option>
              {WEATHER_PRESETS.map((p) => (
                <option key={p.id} value={p.id} className="bg-bg text-fg">
                  {p.label}
                </option>
              ))}
            </select>
          </label>
        </div>
      </section>
      </>)}

      {part === 'events' && (<>
      <section aria-label="Events">
        <h2 className="lbl pt-4">Events</h2>
        {events.length === 0 && (
          <Empty>
            None yet. An event is a script that runs over time: banners and messages, weather, waves of bots and a named boss, a fight to win in time, rewards for those who were there. Start it by hand, on a timer, when players gather at its place, at a time of day, or after another event. Try a preset.
          </Empty>
        )}
        {events.map((e, i) => (
          <EventEditor
            key={i}
            e={e}
            others={events.filter((_, k) => k !== i).map((x) => x.name)}
            dup={events.some((x, k) => k !== i && x.name === e.name)}
            game={game}
            live={liveEvents.has(e.name)}
            row={director?.events.find((r) => r.name === e.name)}
            run={director?.runs?.find((r) => r.name === e.name)}
            set={(n) => setEvent(i, n)}
            rename={(name) => setGame(renameEvent(game, i, name))}
            remove={() => setGame(removeEvent(game, i))}
            post={post}
            lists={{ weather: weather.map((w) => w.name), bots: botNames, places: placeNames, attrs: attrNames, items: (game.items ?? []).map((x) => x.name), zones: (game.zones ?? []).map((z) => z.name) }}
          />
        ))}
        <div className="flex gap-2 pt-2">
          <button type="button" className="btn" disabled={events.length >= MAX_EVENTS} onClick={() => setGame(addEvent(game))}>
            <Plus size={16} strokeWidth={1.5} aria-hidden />
            Add event
          </button>
          <label className="block">
            <span className="sr-only">Add an event from a preset</span>
            <select className="field cursor-pointer text-fg-3" value="" onChange={(e) => e.target.value && setGame(addEvent(game, e.target.value))}>
              <option value="" className="bg-bg">
                or from a preset…
              </option>
              {EVENT_PRESETS.map((p) => (
                <option key={p.id} value={p.id} className="bg-bg text-fg">
                  {p.label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <p className="m-0 pt-2 text-label text-fg-3">With several zones, each zone runs its own events and timers; fire by hand or accept that a timed event will happen once per zone.</p>
      </section>

      <section aria-label="What happened" className="pt-4">
        <h2 className="lbl">What happened</h2>
        {!director || director.log.length === 0 ? (
          <Empty>Nothing yet.</Empty>
        ) : (
          <ol className="list-none p-0 m-0 max-h-56 overflow-y-auto">
            {director.log.map(([t, text], i) => (
              <li key={`${t}-${i}`} className="cell flex gap-3 text-fg-2">
                <span className="text-fg-3 w-16 shrink-0 num">t{t}</span>
                <span className="whitespace-pre-wrap">{text}</span>
              </li>
            ))}
          </ol>
        )}
      </section>
      </>)}
    </div>
  )
}

function swap<T>(a: T[], i: number, j: number): T[] {
  const b = [...a]
  ;[b[i], b[j]] = [b[j]!, b[i]!]
  return b
}

type Start = 'hand' | 'timer' | 'players' | 'time' | 'after'
const START_OPTS: { value: Start; label: string }[] = [
  { value: 'hand', label: 'by hand' },
  { value: 'timer', label: 'on a timer' },
  { value: 'players', label: 'when players gather' },
  { value: 'time', label: 'at a time of day' },
  { value: 'after', label: 'after another event' },
]
const DAY_PARTS: { value: DayPart; label: string }[] = [
  { value: 'dawn', label: 'dawn (sunrise)' },
  { value: 'day', label: 'midday' },
  { value: 'dusk', label: 'dusk (sunset)' },
  { value: 'night', label: 'midnight' },
]

/** One event: what starts it, where it happens, its steps and what happens if it fails, and how it is doing. */
function EventEditor({
  e,
  others,
  dup,
  game,
  live,
  row,
  run,
  set,
  rename,
  remove,
  post,
  lists,
}: {
  e: EventDef
  others: string[]
  dup: boolean
  game: GameFile
  live: boolean
  row: DirectorState['events'][number] | undefined
  run: RunRow | undefined
  set: (e: EventDef) => void
  rename: (name: string) => void
  remove: () => void
  post: (path: string, body: unknown, ok: string) => Promise<void>
  lists: Lists
}) {
  const start: Start = e.when ? e.when.kind : e.every_secs > 0 ? 'timer' : 'hand'
  const setStart = (k: Start) => {
    const when: When | null =
      k === 'players' ? { kind: 'players', count: 2 } : k === 'time' ? { kind: 'time', at: 'dusk' } : k === 'after' ? { kind: 'after', event: others[0] ?? '', outcome: 'success' } : null
    set({ ...e, when, every_secs: k === 'timer' ? Math.max(e.every_secs, 300) : 0 })
  }
  const failing = (e.fail_steps?.length ?? 0) > 0 || (e.limit_secs ?? 0) > 0 || e.steps.some((s) => s.kind === 'defeat')
  return (
    <Strip>
      <div className="flex items-end gap-2">
        <TextField label="Event" value={e.name} onCommit={rename} className="flex-1" invalid={dup} />
        {run ? (
          <button type="button" className="btn" onClick={() => void post('/api/events/stop', { name: e.name }, `Stopped ${e.name}.`)}>
            <Stop {...ICON} aria-hidden />
            Stop
          </button>
        ) : (
          <button type="button" className="btn" disabled={!live} title={live ? undefined : 'Apply first'} onClick={() => void post('/api/events/fire', { name: e.name }, `Started ${e.name}.`)}>
            <Lightning {...ICON} aria-hidden />
            Start now
          </button>
        )}
        <IconButton label={`Remove ${e.name}`} onClick={remove}>
          <Trash2 size={16} strokeWidth={1.5} aria-hidden />
        </IconButton>
      </div>
      <div className="grid grid-cols-3 gap-2 pt-1">
        <SelectField label="Starts" value={start} options={START_OPTS.filter((o) => o.value !== 'after' || others.length > 0)} onChange={setStart} />
        <SelectField label="Where" value={e.place ?? ''} options={withNone(lists.places, 'the whole world')} onChange={(place) => set({ ...e, place: place || null, steps: place ? e.steps : e.steps.map((s) => (s.kind === 'announce' ? { ...s, nearby: false } : s)) })} />
        <NumField label="Time limit (s, 0 = none)" value={e.limit_secs ?? 0} onCommit={(n) => set({ ...e, limit_secs: clamp(Math.round(n), 0, 86400) })} />
      </div>
      <div className="grid grid-cols-3 gap-2 pt-1">
        {start === 'timer' && (
          <>
            <NumField label="Every (s)" value={e.every_secs} onCommit={(n) => set({ ...e, every_secs: clamp(Math.round(n), 5, 86400) })} />
            <NumField label="Chance each time (%)" value={e.chance_pct} onCommit={(n) => set({ ...e, chance_pct: clamp(n, 0, 100) })} />
          </>
        )}
        {e.when?.kind === 'players' && (
          <NumField label={e.place ? 'Players at the place' : 'Players online'} value={e.when.count} onCommit={(n) => set({ ...e, when: { kind: 'players', count: clamp(Math.round(n), 1, 10000) } })} />
        )}
        {e.when?.kind === 'time' && <SelectField label="At" value={e.when.at} options={DAY_PARTS} onChange={(at) => set({ ...e, when: { kind: 'time', at } })} />}
        {e.when?.kind === 'after' && (
          <>
            <SelectField label="After" value={e.when.event} options={opts(others)} onChange={(event) => set({ ...e, when: { ...(e.when as Extract<When, { kind: 'after' }>), event } })} />
            <SelectField
              label="Has"
              value={e.when.outcome ?? 'success'}
              options={[
                { value: 'success', label: 'succeeded' },
                { value: 'failure', label: 'failed' },
                { value: 'either', label: 'ended either way' },
              ]}
              onChange={(outcome) => set({ ...e, when: { ...(e.when as Extract<When, { kind: 'after' }>), outcome } })}
            />
          </>
        )}
        {e.when && <NumField label="Not again for (s)" value={e.cooldown_secs ?? 600} onCommit={(n) => set({ ...e, cooldown_secs: clamp(Math.round(n), 0, 86400) })} />}
      </div>
      <p className="m-0 pt-1 text-label text-fg-3" role="status">
        {run ? (
          <span className="text-fg">
            <span className="inline-block w-2 h-2 mr-1 bg-accent" aria-hidden />
            Running{run.failing ? ' (failing)' : ''}: step {Math.min(run.step, run.of)} of {run.of}
            {run.waiting ? `, ${run.waiting}` : ''}, {run.people} player{run.people === 1 ? '' : 's'} taking part{run.left_secs !== null ? `, ${run.left_secs}s to the time limit` : ''}.
          </span>
        ) : row ? (
          <>
            Ran {row.fired} time{row.fired === 1 ? '' : 's'}
            {row.last ? `, last ${row.last}` : ''}
            {row.next_in_secs !== null ? `, next check in ${row.next_in_secs}s` : ''}.
          </>
        ) : (
          'Not applied yet.'
        )}
      </p>
      <StepList title={`Steps of ${e.name}`} steps={e.steps} game={game} lists={lists} local={!!e.place} min={1} onChange={(steps) => set({ ...e, steps })} />
      {failing ? (
        <div className="pt-2">
          <h3 className="lbl">If it fails (a wait to beat its bots, or the time limit, runs out)</h3>
          <StepList title={`If ${e.name} fails`} steps={e.fail_steps ?? []} game={game} lists={lists} local={!!e.place} min={0} onChange={(fail_steps) => set({ ...e, fail_steps })} />
        </div>
      ) : null}
    </Strip>
  )
}

function StepList({ title, steps, game, lists, local, min, onChange }: { title: string; steps: Step[]; game: GameFile; lists: Lists; local: boolean; min: number; onChange: (s: Step[]) => void }) {
  return (
    <>
      <ol className="list-none p-0 m-0 pt-1" aria-label={title}>
        {steps.map((s, k) => (
          <li key={k} className="grid gap-1 py-1 border-t border-line">
            <div className="flex items-end gap-2">
              <span className="lbl w-5 pb-1">{k + 1}</span>
              <SelectField label="Then" value={s.kind} options={STEP_KINDS.map((x) => ({ value: x.kind, label: x.label }))} onChange={(kind) => kind !== s.kind && onChange(steps.map((x, j) => (j === k ? defaultStep(kind, game) : x)))} className="flex-1" />
              <IconButton label="Move step up" disabled={k === 0} onClick={() => onChange(swap(steps, k, k - 1))}>
                <ArrowUp size={16} strokeWidth={1.5} aria-hidden />
              </IconButton>
              <IconButton label="Move step down" disabled={k === steps.length - 1} onClick={() => onChange(swap(steps, k, k + 1))}>
                <ArrowDown size={16} strokeWidth={1.5} aria-hidden />
              </IconButton>
              <IconButton label="Remove step" disabled={steps.length <= min} onClick={() => onChange(steps.filter((_, j) => j !== k))}>
                <Trash2 size={16} strokeWidth={1.5} aria-hidden />
              </IconButton>
            </div>
            <StepFields step={s} onChange={(n) => onChange(steps.map((x, j) => (j === k ? n : x)))} lists={lists} local={local} />
          </li>
        ))}
      </ol>
      <button type="button" className="btn mt-1" disabled={steps.length >= MAX_STEPS} onClick={() => onChange([...steps, defaultStep('announce', game)])}>
        <Plus size={16} strokeWidth={1.5} aria-hidden />
        Add step
      </button>
    </>
  )
}

interface Lists {
  weather: string[]
  bots: string[]
  places: string[]
  attrs: string[]
  items: string[]
  zones: string[]
}

function StepFields({ step, onChange, lists, local }: { step: Step; onChange: (s: Step) => void; lists: Lists; local: boolean }) {
  const { weather, bots, places, attrs, items, zones } = lists
  const here = local ? 'the event’s place' : 'anywhere'
  switch (step.kind) {
    case 'announce':
      return (
        <div className="grid grid-cols-[1fr_auto_auto] gap-2 items-end">
          <TextField label={step.nearby ? 'Message to those nearby' : 'Message to everyone'} value={step.text} onCommit={(text) => onChange({ ...step, text: text.slice(0, 120) })} />
          <Toggle label="Banner" checked={!!step.banner} onChange={(banner) => onChange({ ...step, banner })} />
          {local && <Toggle label="Nearby only" checked={!!step.nearby} onChange={(nearby) => onChange({ ...step, nearby })} />}
        </div>
      )
    case 'weather':
      return (
        <div className="grid grid-cols-[1fr_6rem] gap-2">
          <SelectField label="Weather" value={step.name ?? ''} options={withNone(weather, '(clear the weather)')} onChange={(name) => onChange({ ...step, name: name || null })} />
          <NumField label="For (s)" value={step.duration_secs} onCommit={(n) => onChange({ ...step, duration_secs: clamp(Math.round(n), 1, 86400) })} />
        </div>
      )
    case 'spawn':
      return (
        <div className="grid grid-cols-[1fr_4.5rem_1fr_1fr] gap-2">
          <SelectField label="Bot type" value={step.archetype} options={opts(bots)} onChange={(archetype) => onChange({ ...step, archetype })} />
          <NumField label="How many" value={step.count} onCommit={(n) => onChange({ ...step, count: clamp(Math.round(n), 1, 500) })} />
          <SelectField label="Where" value={step.place ?? ''} options={withNone(places, here)} onChange={(place) => onChange({ ...step, place: place || null })} />
          <TextField label="Name (a boss)" value={step.name ?? ''} onCommit={(n) => onChange({ ...step, name: n.trim().slice(0, 32) || null })} />
        </div>
      )
    case 'hit':
      return (
        <div className="grid grid-cols-[1fr_4.5rem_1fr] gap-2">
          <SelectField label="Attribute" value={step.attr} options={opts(attrs)} onChange={(attr) => onChange({ ...step, attr })} />
          <NumField label="Change by" value={step.delta} onCommit={(delta) => onChange({ ...step, delta })} />
          <SelectField label="Who" value={step.place ?? ''} options={withNone(places, local ? 'at the event’s place' : 'everyone')} onChange={(place) => onChange({ ...step, place: place || null })} />
        </div>
      )
    case 'clear':
      return <SelectField label="Remove every" value={step.archetype} options={opts(bots)} onChange={(archetype) => onChange({ ...step, archetype })} />
    case 'drop':
      return (
        <div className="grid grid-cols-[1fr_4.5rem_1fr] gap-2">
          <SelectField label="Item" value={step.item} options={opts(items)} onChange={(item) => onChange({ ...step, item })} />
          <NumField label="How many" value={step.count} onCommit={(n) => onChange({ ...step, count: clamp(Math.round(n), 1, 500) })} />
          <SelectField label="Where" value={step.place ?? ''} options={withNone(places, here)} onChange={(place) => onChange({ ...step, place: place || null })} />
        </div>
      )
    case 'wait':
      return <NumField label="Wait (s)" value={step.secs} onCommit={(n) => onChange({ ...step, secs: clamp(Math.round(n), 1, 86400) })} className="w-40" />
    case 'defeat':
      return (
        <NumField label="Within (s, 0 = no limit), or it fails" value={step.within_secs} onCommit={(n) => onChange({ ...step, within_secs: clamp(Math.round(n), 0, 86400) })} className="w-64" />
      )
    case 'reward':
      return (
        <div className="grid grid-cols-[1fr_4.5rem_auto] gap-2 items-end">
          <SelectField label={step.everyone ? 'Give everyone online' : 'Give those who took part'} value={step.item} options={opts(items)} onChange={(item) => onChange({ ...step, item })} />
          <NumField label="How many" value={step.count} onCommit={(n) => onChange({ ...step, count: clamp(Math.round(n), 1, 255) })} />
          <Toggle label="Everyone" checked={!!step.everyone} onChange={(everyone) => onChange({ ...step, everyone })} />
        </div>
      )
    case 'zone':
      return (
        <div className="grid grid-cols-[1fr_8rem] gap-2">
          <SelectField label="Weather zone" value={step.zone} options={opts(zones)} onChange={(zone) => onChange({ ...step, zone })} />
          <SelectField label="Switch it" value={step.on ? 'on' : 'off'} options={[{ value: 'on', label: 'on (raise)' }, { value: 'off', label: 'off (lift)' }]} onChange={(v) => onChange({ ...step, on: v === 'on' })} />
        </div>
      )
  }
}

/** Weather zones: a weather that stays over one place. Defined here (applied with everything else); switched on and
 * off live with the buttons, or by an event. */
function ZonesSection({
  game,
  live,
  setGame,
  director,
  post,
}: {
  game: GameFile
  live: GameFile | null
  setGame: (g: GameFile) => void
  director: DirectorState | undefined
  post: (path: string, body: unknown, ok: string) => Promise<void>
}) {
  const zones = game.zones ?? []
  const liveZones = new Set((live?.zones ?? []).map((z) => z.name))
  const on = new Map((director?.zones ?? []).map((z) => [z.name, z.on]))
  const places = game.places.map((p) => p.name)
  const weathers = (game.weather ?? []).map((w) => w.name)
  const set = (i: number, z: WeatherZone) => setGame({ ...game, zones: zones.map((x, k) => (k === i ? z : x)) })
  return (
    <section aria-label="Weather zones" className="cell py-2">
      <div className="flex items-center gap-2 pb-1">
        <h2 className="lbl m-0">Weather zones</h2>
        <button type="button" className="btn ml-auto" disabled={!places.length || !weathers.length || zones.length >= MAX_ZONES} onClick={() => setGame(addZone(game))}>
          <Plus size={16} strokeWidth={1.5} aria-hidden />
          Add zone
        </button>
      </div>
      {zones.length === 0 ? (
        <p className="m-0 text-fg-3">
          {!places.length || !weathers.length
            ? 'A zone is a weather over a place: make a place (Builder) and a weather (below) first.'
            : 'None yet. A zone is a weather that stays over one place: red mist round a ruin, green mist hiding a castle, a small spring of healing light.'}
        </p>
      ) : (
        <ul className="m-0 p-0 list-none grid gap-2">
          {zones.map((z, i) => (
            <li key={i} className="grid grid-cols-[1fr_1fr_1fr_4.5rem_auto_auto] gap-2 items-end">
              <TextField label="Zone" value={z.name} onCommit={(name) => name.trim() && setGame(renameZone(game, i, name.trim().slice(0, 32)))} />
              <SelectField label="Over place" value={z.place} options={opts(places)} onChange={(place) => set(i, { ...z, place })} />
              <SelectField label="Weather" value={z.weather} options={opts(weathers)} onChange={(weather) => set(i, { ...z, weather })} />
              <NumField label="Height" value={z.height ?? 30} onCommit={(n) => set(i, { ...z, height: clamp(n, 2, 500) })} />
              {liveZones.has(z.name) ? (
                <button
                  type="button"
                  className="btn w-[5.5rem] justify-center"
                  aria-pressed={on.get(z.name) ?? false}
                  aria-label={`${z.name}: ${on.get(z.name) ? 'on, click to lift it' : 'off, click to raise it'}`}
                  title={on.get(z.name) ? 'On now: click to lift it' : 'Off now: click to raise it'}
                  onClick={() => void post('/api/zone', { name: z.name, on: !on.get(z.name) }, `${z.name} ${on.get(z.name) ? 'lifted' : 'raised'}.`)}
                >
                  <span className={`inline-block h-2 w-2 ${on.get(z.name) ? 'bg-accent' : 'bg-fg-3'}`} aria-hidden />
                  {on.get(z.name) ? 'On' : 'Off'}
                </button>
              ) : (
                <span className="text-label text-fg-3 pb-1">apply first</span>
              )}
              <IconButton label={`Remove zone ${z.name}`} onClick={() => setGame(removeZone(game, i))}>
                <Trash2 size={16} strokeWidth={1.5} aria-hidden />
              </IconButton>
            </li>
          ))}
        </ul>
      )}
      <p className="m-0 pt-1 text-label text-fg-3">
        Inside a zone its weather is in force (drains, how far bots see) and players see it; outside it is mist standing over the place. A weather with a positive drain heals. On/Off is live; whether a zone starts on is saved with Apply.
      </p>
    </section>
  )
}

/** When it is light and dark: where the day is now (the schedule that runs), the buttons that move it, and the
 * schedule itself (a draft, applied with everything else). */
function DaylightSection({
  game,
  live,
  setGame,
  setMsg,
  onDaylight,
}: {
  game: GameFile
  live: GameFile | null
  setGame: (g: GameFile) => void
  setMsg: (m: string | null) => void
  onDaylight: (d: Daylight) => void
}) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(t)
  }, [])
  const running = live?.daylight ?? DEFAULT_DAYLIGHT
  const d = game.daylight ?? DEFAULT_DAYLIGHT
  const p = phase(running, now)
  const night = nightShare(running)
  const bad = problem(d)
  const set = (next: Daylight) => setGame({ ...game, daylight: next })
  const jump = async (at: 'dawn' | 'noon' | 'dusk' | 'midnight') => {
    const r = await api<{ daylight?: Daylight; error?: string }>('/api/daylight', 'POST', { phase: moment(running, at) })
    if (r.status === 200 && r.body.daylight) {
      onDaylight(r.body.daylight)
      setMsg(`It is ${at} now.`)
    } else setMsg(r.body.error ?? `the server said ${r.status}`)
  }
  const mode = (m: string) =>
    set(
      m === 'clock'
        ? { mode: 'clock', sunrise: '06:30', sunset: '19:30', utc_offset_minutes: -new Date().getTimezoneOffset() }
        : m === 'fixed'
          ? { mode: 'fixed', phase: Number(p.toFixed(3)) }
          : DEFAULT_DAYLIGHT,
    )
  const hm = /^\d{1,2}:\d{2}$/
  return (
    <section aria-label="Daylight" className="cell py-2">
      <h2 className="lbl pb-1">Day and night</h2>
      <p className="m-0" role="status">
        Now: <strong>{describe(running, p)}</strong>
        <span className="text-fg-3">
          {' '}
          · phase {p.toFixed(3)} · {Math.round(night * 100)}% of the day is night
          {running.mode === 'cycle' ? ` · a day lasts ${running.day_minutes} min` : running.mode === 'clock' ? ` · sun ${running.sunrise}–${running.sunset}` : ' · held'}
        </span>
      </p>
      <div className="flex flex-wrap items-center gap-2 pt-2">
        {(['dawn', 'noon', 'dusk', 'midnight'] as const).map((at) => (
          <button key={at} type="button" className="btn" disabled={running.mode === 'clock'} onClick={() => void jump(at)}>
            {at === 'midnight' ? <Moon {...ICON} aria-hidden /> : at === 'noon' ? <Sun {...ICON} aria-hidden /> : <SunHorizon {...ICON} aria-hidden />}
            {at[0]!.toUpperCase() + at.slice(1)} now
          </button>
        ))}
        {running.mode === 'clock' && <span className="text-label text-fg-3">A clock schedule follows real time.</span>}
      </div>
      <div className="grid grid-cols-[8rem_1fr_1fr_1fr] gap-2 items-end pt-2">
        <SelectField
          label="Schedule"
          value={d.mode}
          options={[
            { value: 'cycle', label: 'cycle' },
            { value: 'clock', label: 'real clock' },
            { value: 'fixed', label: 'held' },
          ]}
          onChange={mode}
        />
        {d.mode === 'cycle' && (
          <>
            <NumField label="Day lasts (min)" value={d.day_minutes} onCommit={(n) => set({ ...d, day_minutes: clamp(n, 1, 10080) })} />
            <NumField label="Night (% of day)" value={Math.round(d.night_share * 100)} onCommit={(n) => set({ ...d, night_share: clamp(n, 0, 90) / 100 })} />
            <span />
          </>
        )}
        {d.mode === 'clock' && (
          <>
            <TextField label="Sunrise" value={d.sunrise} onCommit={(v) => set({ ...d, sunrise: v })} invalid={!hm.test(d.sunrise)} />
            <TextField label="Sunset" value={d.sunset} onCommit={(v) => set({ ...d, sunset: v })} invalid={!hm.test(d.sunset)} />
            <NumField label="UTC offset (min)" value={d.utc_offset_minutes ?? 0} onCommit={(n) => set({ ...d, utc_offset_minutes: clamp(Math.round(n), -840, 840) })} />
          </>
        )}
        {d.mode === 'fixed' && (
          <>
            <NumField label="Moment (0 dawn .. 1)" step={0.01} value={d.phase} onCommit={(n) => set({ ...d, phase: clamp(n, 0, 0.999) })} />
            <span className="text-label text-fg-3 pb-1 col-span-2">
              {describe(d, d.phase)}: noon is {moment(d, 'noon').toFixed(2)}, midnight {moment(d, 'midnight').toFixed(2)}
            </span>
          </>
        )}
      </div>
      {bad && (
        <p role="alert" className="m-0 pt-1 text-accent">
          {bad}
        </p>
      )}
      <p className="m-0 pt-1 text-label text-fg-3">
        Clients work the time out from the schedule and their own clock. Lamps, fires and torches light up as it gets dark. A new schedule takes effect when you press Apply; nobody is disconnected.
      </p>
    </section>
  )
}
