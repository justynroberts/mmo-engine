import { CloudFog, CloudLightning, Crosshair, Lightning, Sun } from '@phosphor-icons/react'
import { ArrowDown, ArrowUp, Plus, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { api, type DirectorState, type EventDef, type GameFile, type Step, type WeatherDef } from '../api'
import {
  addEvent,
  addWeather,
  defaultStep,
  directorProblems,
  EVENT_PRESETS,
  MAX_EVENTS,
  MAX_STEPS,
  MAX_WEATHER,
  removeWeather,
  renameWeather,
  STEP_KINDS,
  WEATHER_PRESETS,
} from '../directorEdit'
import { clamp } from '../gameEdit'
import { Empty, NumField, SelectField, Strip, TextField } from './Fields'

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

export default function God({ game, live, setGame, director, drop, setDrop, note }: Props) {
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

      <section aria-label="Events">
        <h2 className="lbl pt-4">Events</h2>
        {events.length === 0 && <Empty>None yet. An event is a list of things that happen together: announce, change the weather, spawn bots, hit an area. Fire it by hand or let it run on a timer.</Empty>}
        {events.map((e, i) => {
          const row = director?.events.find((r) => r.name === e.name)
          return (
            <Strip key={i}>
              <div className="flex items-end gap-2">
                <TextField label="Event" value={e.name} onCommit={(name) => setEvent(i, { ...e, name })} className="flex-1" invalid={events.some((x, k) => k !== i && x.name === e.name)} />
                <button type="button" className="btn" disabled={!liveEvents.has(e.name)} onClick={() => void post('/api/events/fire', { name: e.name }, `Fired ${e.name}.`)}>
                  <Lightning {...ICON} aria-hidden />
                  Fire now
                </button>
                <IconButton label={`Remove ${e.name}`} onClick={() => setGame({ ...game, events: events.filter((_, k) => k !== i) })}>
                  <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                </IconButton>
              </div>
              <div className="grid grid-cols-2 gap-2 pt-1">
                <NumField label="Repeats (s, 0 = manual)" value={e.every_secs} onCommit={(n) => setEvent(i, { ...e, every_secs: n <= 0 ? 0 : clamp(Math.round(n), 5, 86400) })} />
                <NumField label="Chance each time (%)" value={e.chance_pct} onCommit={(n) => setEvent(i, { ...e, chance_pct: clamp(n, 0, 100) })} />
              </div>
              {row && (
                <p className="m-0 pt-1 text-label text-fg-3">
                  Ran {row.fired} time{row.fired === 1 ? '' : 's'}
                  {row.next_in_secs !== null ? `, next check in ${row.next_in_secs}s` : ''}.
                </p>
              )}
              <ol className="list-none p-0 m-0 pt-1" aria-label={`Steps of ${e.name}`}>
                {e.steps.map((s, k) => (
                  <li key={k} className="grid gap-1 py-1 border-t border-line">
                    <div className="flex items-end gap-2">
                      <span className="lbl w-5 pb-1">{k + 1}</span>
                      <SelectField label="Then" value={s.kind} options={STEP_KINDS.map((x) => ({ value: x.kind, label: x.label }))} onChange={(kind) => kind !== s.kind && setEvent(i, { ...e, steps: e.steps.map((x, j) => (j === k ? defaultStep(kind, game) : x)) })} className="flex-1" />
                      <IconButton label="Move step up" disabled={k === 0} onClick={() => setEvent(i, { ...e, steps: swap(e.steps, k, k - 1) })}>
                        <ArrowUp size={16} strokeWidth={1.5} aria-hidden />
                      </IconButton>
                      <IconButton label="Move step down" disabled={k === e.steps.length - 1} onClick={() => setEvent(i, { ...e, steps: swap(e.steps, k, k + 1) })}>
                        <ArrowDown size={16} strokeWidth={1.5} aria-hidden />
                      </IconButton>
                      <IconButton label="Remove step" disabled={e.steps.length === 1} onClick={() => setEvent(i, { ...e, steps: e.steps.filter((_, j) => j !== k) })}>
                        <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                      </IconButton>
                    </div>
                    <StepFields step={s} onChange={(n) => setEvent(i, { ...e, steps: e.steps.map((x, j) => (j === k ? n : x)) })} weather={weather.map((w) => w.name)} bots={botNames} places={placeNames} attrs={attrNames} items={(game.items ?? []).map((i) => i.name)} />
                  </li>
                ))}
              </ol>
              <button type="button" className="btn mt-1" disabled={e.steps.length >= MAX_STEPS} onClick={() => setEvent(i, { ...e, steps: [...e.steps, defaultStep('announce', game)] })}>
                <Plus size={16} strokeWidth={1.5} aria-hidden />
                Add step
              </button>
            </Strip>
          )
        })}
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
    </div>
  )
}

function swap<T>(a: T[], i: number, j: number): T[] {
  const b = [...a]
  ;[b[i], b[j]] = [b[j]!, b[i]!]
  return b
}

function StepFields({ step, onChange, weather, bots, places, attrs, items }: { step: Step; onChange: (s: Step) => void; weather: string[]; bots: string[]; places: string[]; attrs: string[]; items: string[] }) {
  switch (step.kind) {
    case 'announce':
      return <TextField label="Message to everyone" value={step.text} onCommit={(text) => onChange({ ...step, text: text.slice(0, 120) })} />
    case 'weather':
      return (
        <div className="grid grid-cols-[1fr_6rem] gap-2">
          <SelectField label="Weather" value={step.name ?? ''} options={withNone(weather, '(clear the weather)')} onChange={(name) => onChange({ ...step, name: name || null })} />
          <NumField label="For (s)" value={step.duration_secs} onCommit={(n) => onChange({ ...step, duration_secs: clamp(Math.round(n), 1, 86400) })} />
        </div>
      )
    case 'spawn':
      return (
        <div className="grid grid-cols-[1fr_4.5rem_1fr] gap-2">
          <SelectField label="Bot type" value={step.archetype} options={opts(bots)} onChange={(archetype) => onChange({ ...step, archetype })} />
          <NumField label="How many" value={step.count} onCommit={(n) => onChange({ ...step, count: clamp(Math.round(n), 1, 500) })} />
          <SelectField label="Where" value={step.place ?? ''} options={withNone(places, 'anywhere')} onChange={(place) => onChange({ ...step, place: place || null })} />
        </div>
      )
    case 'hit':
      return (
        <div className="grid grid-cols-[1fr_4.5rem_1fr] gap-2">
          <SelectField label="Attribute" value={step.attr} options={opts(attrs)} onChange={(attr) => onChange({ ...step, attr })} />
          <NumField label="Change by" value={step.delta} onCommit={(delta) => onChange({ ...step, delta })} />
          <SelectField label="Who" value={step.place ?? ''} options={withNone(places, 'everyone')} onChange={(place) => onChange({ ...step, place: place || null })} />
        </div>
      )
    case 'clear':
      return <SelectField label="Remove every" value={step.archetype} options={opts(bots)} onChange={(archetype) => onChange({ ...step, archetype })} />
    case 'drop':
      return (
        <div className="grid grid-cols-[1fr_4.5rem_1fr] gap-2">
          <SelectField label="Item" value={step.item} options={opts(items)} onChange={(item) => onChange({ ...step, item })} />
          <NumField label="How many" value={step.count} onCommit={(n) => onChange({ ...step, count: clamp(Math.round(n), 1, 500) })} />
          <SelectField label="Where" value={step.place ?? ''} options={withNone(places, 'anywhere')} onChange={(place) => onChange({ ...step, place: place || null })} />
        </div>
      )
  }
}

