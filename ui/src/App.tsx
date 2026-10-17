import * as Tabs from '@radix-ui/react-tabs'
import { motion, useReducedMotion } from 'framer-motion'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api, type GameFile, type Info, type PutResult, type State, type WorldGeom } from './api'
import { ActionsEditor, AttrsEditor, BotsEditor } from './components/Editors'
import { ConnectPanel, EntityPanel, RawPanel } from './components/Panels'
import { StatusBar } from './components/StatusBar'
import { WorldView } from './components/WorldView'
import { needsReset } from './gameEdit'

const TABS: readonly (readonly [string, string])[] = [
  ['bots', 'Bots'],
  ['attrs', 'Attributes'],
  ['actions', 'Actions'],
  ['entity', 'Entity'],
  ['connect', 'Connect'],
  ['raw', 'JSON'],
]

const sameJson = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

function useTheme() {
  const [dark, setDark] = useState(() => document.documentElement.classList.contains('dark'))
  const toggle = useCallback(() => {
    setDark((d) => {
      const next = !d
      document.documentElement.classList.toggle('dark', next)
      try {
        localStorage.setItem('theme', next ? 'dark' : 'light')
      } catch {
        /* storage can be blocked; the choice just won't persist */
      }
      return next
    })
  }, [])
  return { dark, toggle }
}

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
  const { dark, toggle } = useTheme()
  const reduce = useReducedMotion()
  const stateRef = useRef<State | null>(null)
  const [stats, setStats] = useState<State | null>(null)
  const [online, setOnline] = useState(false)
  const [info, setInfo] = useState<Info | null>(null)
  const [world, setWorld] = useState<WorldGeom | null>(null)
  const [live, setLive] = useState<GameFile | null>(null)
  const [draft, setDraft] = useState<GameFile | null>(null)
  const [raw, setRaw] = useState<{ text: string; error: string | null } | null>(null)
  const [tab, setTab] = useState('bots')
  const [selected, setSelected] = useState<number | null>(null)
  const [possessed, setPossessed] = useState<number | null>(null)
  const [follow, setFollow] = useState(false)
  const [applying, setApplying] = useState(false)
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null)

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
            : 'Applied live. Connected players were not affected.',
        })
        setPossessed(null)
        setSelected(null)
        await loadGame()
        if (r.body.reset) await loadStatic()
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
  const body = (child: React.ReactNode) => <motion.div {...fade}>{child}</motion.div>

  return (
    <div className="min-h-full lg:h-screen flex flex-col">
      <StatusBar s={stats} online={online} dark={dark} onTheme={toggle} />
      <main className="flex-1 lg:min-h-0 grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_34rem] grid-rows-[min(60vh,420px)_auto] lg:grid-rows-1">
        <WorldView
          world={world}
          stateRef={stateRef}
          colors={live?.archetypes.map((a) => a.color) ?? []}
          selected={selected}
          onSelect={(id) => {
            setSelected(id)
            if (id !== null) setTab('entity')
          }}
          follow={follow}
          interest={live?.interest_radius ?? 128}
          healthIdx={live?.attrs.some((a) => a.name === 'health') ? healthIdx : -1}
          healthMax={healthDef?.max ?? 100}
          themeKey={dark ? 'dark' : 'light'}
        />
        <aside className="flex flex-col lg:min-h-0 border-t lg:border-t-0 lg:border-l border-line">
          <Tabs.Root
            value={tab}
            onValueChange={(v) => {
              // Half-typed JSON is dropped when you leave the JSON tab; the draft keeps its last valid state.
              if (v !== 'raw') setRaw(null)
              setTab(v)
            }} className="flex flex-col lg:min-h-0 lg:flex-1">
            <Tabs.List aria-label="Sections" className="cell flex px-2 overflow-x-auto shrink-0">
              {TABS.map(([v, l]) => (
                <Tabs.Trigger key={v} value={v} className="tab">
                  {l}
                </Tabs.Trigger>
              ))}
            </Tabs.List>
            <div className="lg:flex-1 lg:min-h-0 lg:overflow-y-auto px-4 pb-4">
              <Tabs.Content value="bots" tabIndex={-1}>
                {body(
                  draft ? (
                    <BotsEditor game={draft} setGame={setGame} liveCounts={liveCounts} onSpawn={spawn} onKillAll={killAll} />
                  ) : (
                    <p className="text-fg-3 py-4">Loading game file…</p>
                  ),
                )}
              </Tabs.Content>
              <Tabs.Content value="attrs" tabIndex={-1}>
                {body(draft && <AttrsEditor game={draft} setGame={setGame} />)}
              </Tabs.Content>
              <Tabs.Content value="actions" tabIndex={-1}>
                {body(draft && <ActionsEditor game={draft} setGame={setGame} />)}
              </Tabs.Content>
              <Tabs.Content value="entity" tabIndex={-1}>
                {body(
                  <EntityPanel
                    s={stats}
                    game={live}
                    selected={selected}
                    possessed={possessed}
                    follow={follow}
                    onFollow={setFollow}
                    onSelect={setSelected}
                    onPossess={setPossessed}
                    onKill={kill}
                    actionName={firstAction}
                  />,
                )}
              </Tabs.Content>
              <Tabs.Content value="connect" tabIndex={-1}>
                {body(<ConnectPanel info={info} />)}
              </Tabs.Content>
              <Tabs.Content value="raw" tabIndex={-1}>
                {body(
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
                  />,
                )}
              </Tabs.Content>
            </div>
          </Tabs.Root>
          <footer className="border-t border-line px-4 py-2 shrink-0 sticky bottom-0 bg-bg" aria-label="Apply changes">
            <div className="flex items-center gap-2">
              <button type="button" className={`btn ${dirty ? 'btn-primary' : ''}`} disabled={!dirty || applying} onClick={() => void apply()}>
                {applying ? 'Applying…' : 'Apply'}
              </button>
              <button type="button" className="btn" disabled={!dirty || applying} onClick={() => { setDraft(live); setRaw(null); setResult(null) }}>
                Revert
              </button>
              <span className="text-fg-3 truncate">
                {dirty ? (reset ? 'Changes the wire format: restarts the world' : 'Bot changes only: applies live') : 'In sync with the server'}
              </span>
            </div>
            <div aria-live="polite" className="min-h-6">
              {result && (
                <p role={result.ok ? 'status' : 'alert'} className={result.ok ? 'text-fg-2' : 'text-accent'}>
                  {result.text}
                </p>
              )}
            </div>
          </footer>
        </aside>
      </main>
    </div>
  )
}
