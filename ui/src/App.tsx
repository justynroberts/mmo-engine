import * as Tabs from '@radix-ui/react-tabs'
import { CloudSun, HardDrives, Lightning, MapTrifold, Pulse, Robot, SlidersHorizontal, type Icon } from '@phosphor-icons/react'
import God, { type Drop } from './components/God'
import { motion, useReducedMotion } from 'framer-motion'
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api, type GameFile, type Info, type PutResult, type State, type WorldGeom } from './api'
import { Builder, initialBuilder, type BuilderState } from './components/Builder'
import { ChatPanel } from './components/Chat'
import { ActionsEditor, AttrsEditor, BotsEditor, ItemsEditor, QuestsEditor } from './components/Editors'
import { ConnectPanel, EntityPanel, RawPanel } from './components/Panels'
import { Header, NetworkPanel } from './components/StatusBar'
import type { Daylight } from './daylight'
import { WorldView } from './components/WorldView'
import { WorldTab, useShaper } from './components/WorldTab'
import type { BlockCol } from './components/mapLook'
import { parseWorld, type WorldData } from './world'
const Scene3D = lazy(() => import('./components/Scene3D'))
import { addWall, needsReset, newPlace, newRoute, normWall } from './gameEdit'

/** The console's sections, by what a game master does: each says what is inside it, and the ones that work on the
 * map show it beside them. */
type Section = 'live' | 'world' | 'bots' | 'sky' | 'events' | 'rules' | 'server'
const SECTIONS: readonly { id: Section; name: string; what: string; icon: Icon; map: boolean; title: string; help: string }[] = [
  { id: 'live', name: 'Live', what: 'Map, who is on, chat', icon: Pulse, map: true, title: 'The world right now', help: 'Click anyone on the map to see their attributes, follow them, take control of a bot or remove it. Chat shows what is being said.' },
  { id: 'world', name: 'World', what: 'Terrain, places, clear down', icon: MapTrifold, map: true, title: 'The world', help: 'Make the land from a recipe and apply it live, mark places and routes for bots, and clear away what players leave behind.' },
  { id: 'bots', name: 'Bots', what: 'Types, spawning, loot', icon: Robot, map: true, title: 'Bots', help: 'What kinds of bot there are, how they behave and what they drop, and putting more of them into the world.' },
  { id: 'sky', name: 'Sky', what: 'Day, weather, zones', icon: CloudSun, map: false, title: 'Sky and weather', help: 'When it is light and dark, the weather everywhere, and weather that stays over one place: mist, storms, a healing spring.' },
  { id: 'events', name: 'Events', what: 'What happens, when', icon: Lightning, map: false, title: 'Events', help: 'World and local events, raids and bosses: scripts that run over time, with banners, waves, a fight to win in time and rewards. Started by hand, on a timer, when players gather, at a time of day, or after another event.' },
  { id: 'rules', name: 'Rules', what: 'Attributes, actions, items, quests', icon: SlidersHorizontal, map: false, title: 'Game rules', help: 'What every character has (health, energy), what they can do to each other, the items in the world and the quests bots give. Some changes restart the world: the header says when.' },
  { id: 'server', name: 'Server', what: 'Connect, network, JSON', icon: HardDrives, map: false, title: 'Server', help: 'How players connect, how the server is coping, and the whole game file as JSON.' },
]
/** Pages within a section, where it has more than one. */
const SUBS: Partial<Record<Section, readonly (readonly [string, string])[]>> = {
  live: [['inspect', 'Inspect'], ['chat', 'Chat']],
  world: [['terrain', 'Terrain'], ['build', 'Places, routes and walls']],
  bots: [['types', 'Bot types'], ['spawn', 'Spawn and scatter']],
  rules: [['attrs', 'Attributes'], ['actions', 'Actions'], ['items', 'Items'], ['quests', 'Quests']],
  server: [['connect', 'Connect'], ['network', 'Network'], ['json', 'Game file (JSON)']],
}

const sameJson = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

/** Poll the server's state a few times a second without piling up requests. */
function useState_(onState: (s: State | null) => void) {
  useEffect(() => {
    let stop = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const ctl = new AbortController()
    const tick = async () => {
      try {
        const r = await api<State>('/api/state?limit=6000', 'GET', undefined, ctl.signal)
        onState(r.status === 200 ? r.body : null)
      } catch {
        if (!stop) onState(null)
      }
      if (!stop) timer = setTimeout(tick, 200)
    }
    void tick()
    return () => {
      stop = true
      ctl.abort()
      if (timer) clearTimeout(timer)
    }
  }, [onState])
}

/** While an entity is possessed, turn WASD/arrows/space into drive commands. */
function useDrive(id: number | null, action: string | undefined, onLost: () => void) {
  useEffect(() => {
    if (id === null) return
    const keys = new Set<string>()
    const typing = (t: EventTarget | null) =>
      t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT')
    const down = (e: KeyboardEvent) => {
      if (typing(e.target) || e.ctrlKey || e.metaKey || e.altKey) return
      const k = e.key.toLowerCase()
      if (['w', 'a', 's', 'd', 'arrowup', 'arrowdown', 'arrowleft', 'arrowright', ' '].includes(k)) {
        keys.add(k)
        e.preventDefault()
      }
    }
    const up = (e: KeyboardEvent) => keys.delete(e.key.toLowerCase())
    const blur = () => keys.clear()
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    window.addEventListener('blur', blur)
    let last = ''
    let sinceSend = 0
    const timer = setInterval(async () => {
      const dx = (keys.has('d') || keys.has('arrowright') ? 1 : 0) - (keys.has('a') || keys.has('arrowleft') ? 1 : 0)
      const dy = (keys.has('s') || keys.has('arrowdown') ? 1 : 0) - (keys.has('w') || keys.has('arrowup') ? 1 : 0)
      const act = keys.has(' ') && action ? action : null
      const sig = `${dx},${dy},${act}`
      sinceSend += 100
      if (sig === last && !act && sinceSend < 1000) return
      last = sig
      sinceSend = 0
      try {
        const r = await api('/api/drive', 'POST', { id, dx, dy, action: act })
        if (r.status === 404) onLost()
      } catch {
        /* server gone; the status bar says so */
      }
    }, 100)
    return () => {
      clearInterval(timer)
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
      window.removeEventListener('blur', blur)
      void api('/api/release', 'POST', { id }).catch(() => undefined)
    }
  }, [id, action, onLost])
}

export default function App() {
  const dark = true // the console is near-black only
  const reduce = useReducedMotion()
  const stateRef = useRef<State | null>(null)
  const [stats, setStats] = useState<State | null>(null)
  const [online, setOnline] = useState(false)
  const [info, setInfo] = useState<Info | null>(null)
  const [world, setWorld] = useState<WorldGeom | null>(null)
  /** The world file (terrain, water, objects), when the server has one; fetched again when its version changes. */
  const [land, setLand] = useState<WorldData | null>(null)
  const [live, setLive] = useState<GameFile | null>(null)
  const [draft, setDraft] = useState<GameFile | null>(null)
  const [raw, setRaw] = useState<{ text: string; error: string | null } | null>(null)
  // Where we are: a section and, for those that have them, a page within it (kept in the address, #bots/spawn).
  const [section, setSectionState] = useState<Section>(() => {
    const h = location.hash.slice(1).split('/')[0] as Section
    return SECTIONS.some((x) => x.id === h) ? h : 'live'
  })
  const [subs, setSubs] = useState<Partial<Record<Section, string>>>(() => {
    const [h, p] = location.hash.slice(1).split('/')
    return h && p ? { [h as Section]: p } : {}
  })
  const subOf = (id: Section) => subs[id] ?? SUBS[id]?.[0]?.[0] ?? ''
  const setSub = (id: Section, v: string) => setSubs((cur) => ({ ...cur, [id]: v }))
  const setSection = (id: Section) => setSectionState(id)
  useEffect(() => {
    const p = subs[section]
    const h = `#${section}${p ? `/${p}` : ''}`
    if (location.hash !== h) history.pushState(null, '', h) // so Back goes to the last section
  }, [section, subs])
  // Back, forward and pasted links move between sections too.
  useEffect(() => {
    const on = () => {
      const [h, p] = location.hash.slice(1).split('/')
      if (SECTIONS.some((x) => x.id === h)) {
        setSectionState(h as Section)
        if (p) setSubs((cur) => ({ ...cur, [h as Section]: p }))
      }
    }
    window.addEventListener('hashchange', on)
    return () => window.removeEventListener('hashchange', on)
  }, [])
  // The map's edit modes, as they were named before the console had sections.
  const tab = section === 'world' ? (subOf('world') === 'build' ? 'builder' : 'world') : section === 'bots' ? 'god' : section
  const onDaylight = (d: Daylight) => {
    setLive((l) => (l ? { ...l, daylight: d } : l))
    setDraft((g) => (g ? { ...g, daylight: d } : g))
  }
  const [selected, setSelected] = useState<number | null>(null)
  const [possessed, setPossessed] = useState<number | null>(null)
  const [follow, setFollow] = useState(false)
  const [view3d, setView3d] = useState(false)
  const [applying, setApplying] = useState(false)
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null)
  const [b, setB] = useState<BuilderState>(initialBuilder)
  const [note, setNote] = useState<string | null>(null)
  const [drop, setDrop] = useState<Drop | null>(null)

  const onState = useCallback((s: State | null) => {
    stateRef.current = s
    setOnline(s !== null)
    if (s) setStats(s)
  }, [])
  useState_(onState)

  const loadGame = useCallback(async () => {
    const r = await api<GameFile>('/api/game')
    if (r.status === 200) {
      setLive(r.body)
      setDraft(r.body)
      setRaw(null)
    }
  }, [])
  const loadStatic = useCallback(async () => {
    const [i, w] = await Promise.all([api<Info>('/api/info'), api<WorldGeom>('/api/world')])
    if (i.status === 200) setInfo(i.body)
    if (w.status === 200) setWorld(w.body)
  }, [])
  useEffect(() => {
    void loadGame()
    void loadStatic()
  }, [loadGame, loadStatic])

  const shaper = useShaper(land, world?.size ?? 2048, info?.seed ?? 1)
  // Blocks players have built, fetched again whenever they change.
  const [blocks, setBlocks] = useState<BlockCol[]>([])
  const blocksVer = stats?.blocks?.ver
  useEffect(() => {
    if (blocksVer === undefined) return
    let cancelled = false
    void api<{ cols: BlockCol[] }>('/api/blocks').then((r) => {
      if (!cancelled && r.status === 200) setBlocks(r.body.cols)
    })
    return () => {
      cancelled = true
    }
  }, [blocksVer])
  // The World tab shows its preview on the map, and takes edits there.
  const shown = (tab === 'world' && shaper.preview) || land
  const worldVer = stats?.world_ver
  useEffect(() => {
    if (!worldVer) {
      setLand(null)
      return
    }
    let cancelled = false
    void fetch('/api/worldfile')
      .then((r) => (r.ok ? r.text() : ''))
      .then((t) => {
        if (!cancelled) setLand(t ? parseWorld(t) : null)
      })
      .catch(() => undefined)
    void loadStatic() // a reloaded world file can bring new walls
    return () => {
      cancelled = true
    }
  }, [worldVer, loadStatic])

  // After a server restart the hash changes; pick up the new game and world.
  const hash = stats?.hash
  const lastHash = useRef<string | undefined>(undefined)
  useEffect(() => {
    if (hash && lastHash.current && hash !== lastHash.current) {
      void loadStatic()
      void loadGame()
    }
    if (hash) lastHash.current = hash
  }, [hash, loadGame, loadStatic])

  const dirty = !!live && !!draft && !sameJson(live, draft)
  const reset = !!live && !!draft && needsReset(live, draft)

  const apply = async () => {
    if (!draft) return
    setApplying(true)
    setResult(null)
    try {
      const r = await api<PutResult>('/api/game', 'PUT', draft)
      if (r.status === 200 && r.body.ok) {
        setResult({
          ok: true,
          text: r.body.reset
            ? `Applied. World restarted under new definitions; ${r.body.dropped_clients ?? 0} client(s) dropped.`
            : `Applied live. Connected players were not affected${r.body.moved_entities ? `; ${r.body.moved_entities} moved out of new walls` : ''}.`,
        })
        setPossessed(null)
        setSelected(null)
        await loadGame()
        await loadStatic()
      } else {
        setResult({ ok: false, text: r.body.error ?? `server said ${r.status}` })
      }
    } catch (e) {
      setResult({ ok: false, text: `could not reach the server: ${String(e)}` })
    } finally {
      setApplying(false)
    }
  }

  const setGame = (g: GameFile) => {
    setDraft(g)
    setRaw(null)
    setResult(null)
  }

  const spawn = (name: string, count: number) => void api('/api/spawn', 'POST', { archetype: name, count })
  const killAll = () => void api('/api/kill', 'POST', { all_bots: true }).then(() => { setSelected(null); setPossessed(null) })
  const kill = (id: number) =>
    void api('/api/kill', 'POST', { id }).then(() => {
      setSelected((s) => (s === id ? null : s))
      setPossessed((p) => (p === id ? null : p))
    })

  const finishRoute = () => {
    if (!draft || b.draft.length < 2) return
    const route = newRoute(draft, b.draft)
    setGame({ ...draft, routes: [...draft.routes, route] })
    setB({ ...b, tool: 'select', draft: [], sel: { kind: 'route', index: draft.routes.length } })
  }
  const onMapClick = (x: number, y: number) => {
    if (tab === 'god' && drop) {
      void api<{ error?: string; note?: string }>('/api/spawn', 'POST', { archetype: drop.arch, count: drop.count, x, y, radius: 6 }).then((r) =>
        setNote(r.status === 200 ? r.body.note ?? `Dropped ${drop.count} ${drop.arch} at ${Math.round(x)}, ${Math.round(y)}.` : (r.body.error ?? `server said ${r.status}`)),
      )
      return
    }
    if (!draft) return
    if (b.tool === 'place') {
      setGame({ ...draft, places: [...draft.places, newPlace(draft, x, y)] })
      setB({ ...b, tool: 'select', sel: { kind: 'place', index: draft.places.length } })
    } else if (b.tool === 'route') {
      setB({ ...b, draft: [...b.draft, [Math.round(x), Math.round(y)]] })
    } else if (b.tool === 'spawn') {
      const a = draft.archetypes[b.arch]
      if (!a) return
      void api<{ error?: string; note?: string }>('/api/spawn', 'POST', { archetype: a.name, count: 1, x, y }).then((r) =>
        setNote(r.status === 200 ? r.body.note ?? `Dropped a ${a.name} at ${Math.round(x)}, ${Math.round(y)}.` : (r.body.error ?? `server said ${r.status}`)),
      )
    }
  }

  const firstAction = info?.actions[0]?.name
  const onLost = useCallback(() => setPossessed(null), [])
  useDrive(possessed, firstAction, onLost)

  const liveCounts = useMemo(() => {
    const counts: number[] = []
    for (const r of stats?.e ?? []) {
      const a = r[3] ?? -1
      if (a >= 0) counts[a] = (counts[a] ?? 0) + 1
    }
    return counts
  }, [stats])

  const healthIdx = Math.max(0, live?.attrs.findIndex((a) => a.name === 'health') ?? -1)
  const healthDef = live?.attrs[healthIdx]
  const rawText = raw ? raw.text : draft ? JSON.stringify(draft, null, 2) : ''

  const fade = { initial: { opacity: 0 }, animate: { opacity: 1 }, transition: { duration: reduce ? 0 : 0.15, ease: 'easeOut' as const } }
  const loading = <p className="text-fg-3 py-4">Loading the game file…</p>
  const cur = SECTIONS.find((x) => x.id === section) ?? SECTIONS[0]!
  const sub = subOf(section)
  const go = (id: Section) => {
    // Half-typed JSON is dropped when you leave it; the draft keeps its last valid state.
    if (id !== 'server') setRaw(null)
    if (id !== 'bots') setDrop(null)
    setSection(id)
  }

  const mapView = (
        <div className="relative min-h-0">
  {!view3d ? (
          <WorldView
            edit={
              (tab === 'builder' || (tab === 'god' && drop)) && draft
                ? {
                    places: draft.places,
                    routes: draft.routes,
                    home: draft.archetypes[b.arch]?.home ?? null,
                    sel: b.sel,
                    tool: tab === 'god' ? 'spawn' : b.tool,
                    draft: b.draft,
                    walls: draft.walls ?? null,
                    onAddWall: (x0, y0, x1, y1) => {
                      const w = normWall(x0, y0, x1, y1, world?.size ?? 2048)
                      if (!w) return
                      const base = draft.walls ? draft : { ...draft, walls: (world?.boxes ?? []).map((q) => [...q] as [number, number, number, number]) }
                      setGame(addWall(base, w))
                      setB((cur) => ({ ...cur, sel: { kind: 'wall', index: (base.walls?.length ?? 0) } }))
                    },
                    onPick: (sel) => setB((cur) => ({ ...cur, sel })),
                    onClick: onMapClick,
                    onMovePlace: (i, x, y) => setGame({ ...draft, places: draft.places.map((p, k) => (k === i ? { ...p, x, y } : p)) }),
                    onMovePoint: (r, k, x, y) =>
                      setGame({
                        ...draft,
                        routes: draft.routes.map((rt, ri) => (ri === r ? { ...rt, points: rt.points.map((pt, j) => (j === k ? [x, y] : pt)) } : rt)),
                      }),
                  }
                : null
            }
            world={world}
            stateRef={stateRef}
            colors={live?.archetypes.map((a) => a.color) ?? []}
            selected={selected}
            onSelect={(id) => {
              setSelected(id)
              if (id !== null) {
                setSection('live')
                setSub('live', 'inspect')
              }
            }}
            follow={follow}
            interest={live?.interest_radius ?? 128}
            healthIdx={live?.attrs.some((a) => a.name === 'health') ? healthIdx : -1}
            healthMax={healthDef?.max ?? 100}
            themeKey={dark ? 'dark' : 'light'}
            land={shown}
            blocks={blocks}
            shape={tab === 'world' ? shaper.overlay : null}
          />
          ) : (
            <Suspense fallback={<div className="h-full bg-[#a9c3d6]" />}>
              <Scene3D
                world={world}
                places={live?.places ?? []}
                routes={live?.routes ?? []}
                archetypes={live?.archetypes.map((a) => a.name) ?? []}
                colors={live?.archetypes.map((a) => a.color) ?? []}
                land={shown}
                blocks={blocks}
                airIdx={live?.attrs.findIndex((a) => a.name === 'airborne') ?? -1}
                airMax={live?.attrs.find((a) => a.name === 'airborne')?.max ?? 100}
                healthIdx={live?.attrs.some((a) => a.name === 'health') ? healthIdx : -1}
                healthMax={healthDef?.max ?? 100}
                stateRef={stateRef}
                selected={selected}
                onSelect={(id) => {
                  setSelected(id)
                  if (id !== null) {
                setSection('live')
                setSub('live', 'inspect')
              }
                }}
                follow={follow}
                interest={live?.interest_radius ?? 128}
              />
            </Suspense>
          )}
          <div role="group" aria-label="Map view" className="absolute top-2 right-2 flex border border-line bg-bg">
            {([['2D', false], ['3D', true]] as const).map(([l, v]) => (
              <button key={l} type="button" aria-pressed={view3d === v} onClick={() => setView3d(v)}
                className={`text-label font-semibold h-7 px-3 cursor-pointer border-0 ${view3d === v ? 'bg-accent text-accent-ink' : 'bg-transparent text-fg-2'}`}>
                {l}
              </button>
            ))}
          </div>
        </div>
  )

  const page = (id: Section): React.ReactNode => {
    switch (id) {
      case 'live':
        return sub === 'chat' ? (
          <ChatPanel selected={selected} active={section === 'live'} />
        ) : (
          <EntityPanel s={stats} game={live} selected={selected} possessed={possessed} follow={follow} onFollow={setFollow} onSelect={setSelected} onPossess={setPossessed} onKill={kill} actionName={firstAction} />
        )
      case 'world':
        return sub === 'build' ? (
          draft ? <Builder game={draft} setGame={setGame} b={b} setB={setB} finishRoute={finishRoute} note={note} worldBoxes={world?.boxes ?? []} worldSize={world?.size ?? 2048} worldFile={!!land} /> : loading
        ) : (
          <WorldTab s={shaper} land={land} liveSize={world?.size ?? 2048} blocks={stats?.blocks?.total ?? 0} places={(live?.places ?? []).map((p) => p.name)} />
        )
      case 'bots':
        return draft ? (
          sub === 'spawn' ? (
            <God part="bots" game={draft} live={live} setGame={setGame} director={stats?.director} drop={drop} setDrop={setDrop} note={note} onDaylight={onDaylight} />
          ) : (
            <BotsEditor game={draft} setGame={setGame} liveCounts={liveCounts} onSpawn={spawn} onKillAll={killAll} />
          )
        ) : (
          loading
        )
      case 'sky':
      case 'events':
        return draft ? <God part={id} game={draft} live={live} setGame={setGame} director={stats?.director} drop={drop} setDrop={setDrop} note={note} onDaylight={onDaylight} /> : loading
      case 'rules':
        if (!draft) return loading
        return sub === 'actions' ? (
          <ActionsEditor game={draft} setGame={setGame} />
        ) : sub === 'items' ? (
          <ItemsEditor game={draft} setGame={setGame} />
        ) : sub === 'quests' ? (
          <QuestsEditor game={draft} setGame={setGame} />
        ) : (
          <AttrsEditor game={draft} setGame={setGame} />
        )
      case 'server':
        return sub === 'network' ? (
          <NetworkPanel s={stats} />
        ) : sub === 'json' ? (
          <RawPanel
            text={rawText}
            error={raw?.error ?? null}
            onText={(text) => {
              try {
                const g = JSON.parse(text) as GameFile
                setDraft(g)
                setRaw({ text, error: null })
              } catch (e) {
                setRaw({ text, error: String(e) })
              }
              setResult(null)
            }}
          />
        ) : (
          <ConnectPanel info={info} />
        )
    }
  }

  return (
    <div className="console min-h-full lg:h-screen flex flex-col">
      <Header
        s={stats}
        online={online}
        dirty={dirty}
        restarts={reset}
        applying={applying}
        onApply={() => void apply()}
        onRevert={() => {
          setDraft(live)
          setRaw(null)
          setResult(null)
        }}
        result={result}
      />
      <Tabs.Root value={section} onValueChange={(v) => go(v as Section)} className="flex-1 flex flex-col lg:min-h-0">
        <nav aria-label="Sections" className="shrink-0">
        <Tabs.List aria-label="Sections" className="flex border-b border-line overflow-x-auto">
          {SECTIONS.map((x) => (
            <Tabs.Trigger key={x.id} value={x.id} className="navtab" aria-label={x.name} aria-describedby={`what-${x.id}`}>
              <x.icon size={22} weight={section === x.id ? 'regular' : 'light'} aria-hidden className="shrink-0 mt-0.5" />
              <span className="min-w-0">
                <span className="block font-semibold leading-5">{x.name}</span>
                <span id={`what-${x.id}`} className="what">
                  {x.what}
                </span>
              </span>
            </Tabs.Trigger>
          ))}
        </Tabs.List>
        </nav>
        <main className={`flex-1 lg:min-h-0 ${cur.map ? 'grid grid-cols-1 grid-rows-[min(60vh,440px)_auto] lg:grid-rows-1 lg:grid-cols-[minmax(0,1fr)_34rem]' : 'flex flex-col'}`}>
          {cur.map && mapView}
          {SECTIONS.map((x) => (
            <Tabs.Content
              key={x.id}
              value={x.id}
              tabIndex={-1}
              className={x.map ? 'lg:min-h-0 lg:overflow-y-auto border-t lg:border-t-0 lg:border-l border-line' : 'lg:min-h-0 lg:overflow-y-auto flex-1'}
            >
              <motion.div {...fade} className={`px-5 py-4 grid gap-4 content-start ${x.map ? '' : 'max-w-[900px]'}`}>
                <div className="grid gap-1">
                  <h2 className="page-title">{x.title}</h2>
                  <p className="page-help">{x.help}</p>
                </div>
                {SUBS[x.id] && (
                  <div role="tablist" aria-label={`${x.name} pages`} className="flex border-b border-line -mt-1">
                    {SUBS[x.id]!.map(([v, l]) => (
                      <button
                        key={v}
                        type="button"
                        role="tab"
                        aria-selected={subOf(x.id) === v}
                        data-state={subOf(x.id) === v ? 'active' : 'inactive'}
                        className="tab"
                        onClick={() => setSub(x.id, v)}
                      >
                        {l}
                      </button>
                    ))}
                  </div>
                )}
                <div>{page(x.id)}</div>
              </motion.div>
            </Tabs.Content>
          ))}
        </main>
      </Tabs.Root>
    </div>
  )
}
