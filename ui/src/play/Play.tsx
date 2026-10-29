import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react'
import { setPlayBase } from './base'
import { GameClient, zoneUrl, type PlayInfo, type Status } from './client'
import Hud, { type Line } from './Hud'
import { newShared } from './shared'
import SignIn from './SignIn'
import { channel, recoveryCode, type ChatMsg } from './protocol'
import { playUrl } from './base'

const PlayScene = lazy(() => import('./PlayScene'))

function useTheme() {
  const [dark, setDark] = useState(() => document.documentElement.classList.contains('dark'))
  const toggle = () => {
    const next = !dark
    document.documentElement.classList.toggle('dark', next)
    try {
      localStorage.setItem('theme', next ? 'dark' : 'light')
    } catch {
      /* the choice just won't persist */
    }
    setDark(next)
  }
  return { dark, toggle }
}

/** Walls and weather can change while people play; look now and then so prediction and the sky match the server. */
const WORLD_CHECK_MS = 5000

export default function Play() {
  const { dark, toggle } = useTheme()
  const [info, setInfo] = useState<PlayInfo | null>(null)
  const [infoError, setInfoError] = useState<string | null>(null)
  const [client, setClient] = useState<GameClient | null>(null)
  const [identity, setIdentity] = useState<Uint8Array | null>(null)
  const [status, setStatus] = useState<{ s: Status; detail?: string }>({ s: 'connecting' })
  const [lines, setLines] = useState<Line[]>([])
  /** Zone lines: the zone we are on our way to, while its world loads. */
  const [travelling, setTravelling] = useState<string | null>(null)
  const shared = useRef(newShared())
  const lineKey = useRef(0)

  useEffect(() => {
    fetch(playUrl('/play/info'))
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`the server said ${r.status}`))))
      .then(setInfo)
      .catch((e) => setInfoError(String(e.message ?? e)))
  }, [])

  const enter = (id: Uint8Array | null, name?: string) => {
    if (!info) return
    setIdentity(id)
    const url = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`
    const c = play(info, id, url)
    // Known by the account name; the server says in chat if it is taken or not allowed.
    if (name) c.setName(name)
  }

  /** Start a client on a zone (this page's server, or the one a zone line sent us to). */
  const play = (inf: PlayInfo, id: Uint8Array | null, url: string): GameClient => {
    const names = shared.current.names
    const add = (m: ChatMsg, self: number) =>
      setLines((l) => [...l.slice(-30), { key: lineKey.current++, from: m.from, who: m.from === 0 ? 'server' : m.from === self ? 'you' : names.name(m.from), text: m.channel === channel.EMOTE ? m.text.slice(m.text.indexOf('|') + 1) : m.text, shout: m.channel === 1, banner: m.from === 0 && m.channel === channel.BANNER }])
    const c: GameClient = new GameClient(inf, id, url, {
      onStatus: (s, detail) => setStatus({ s, detail }),
      // Lit windows and a bot's wordless dance are for a 3D client; nothing to say in the chat.
      onChat: (m) => (m.channel === channel.LIT || m.channel === channel.PLAGUE || (m.channel === channel.EMOTE && m.text.endsWith('|')) ? undefined : add(m, c.entity)),
      onMoved: (zone) => void travel(inf, id, zone),
    })
    setClient(c)
    c.start()
    return c
  }

  /** Zone lines: walk through an exit into another zone. Its map, walls and blocks come from its own server, so
   *  read its info there and start again with the same identity; that server has just been handed our character. */
  const travel = async (from: PlayInfo, id: Uint8Array | null, zone: number) => {
    const base = from.zones?.[zone]
    const where = from.zone_names?.[zone] ?? `zone ${zone}`
    if (!base) {
      setStatus({ s: 'closed', detail: `${where} has no web address (the server's --zone-web).` })
      return
    }
    setTravelling(where)
    try {
      setPlayBase(base)
      const r = await fetch(`${base.replace(/\/+$/, '')}/play/info`)
      if (!r.ok) throw new Error(`${where} said ${r.status}`)
      const next = (await r.json()) as PlayInfo
      // Ids, names and blocks are that server's own; nothing here carries over.
      const fresh = newShared()
      fresh.zoom = shared.current.zoom
      shared.current = fresh
      setInfo(next)
      play(next, id, zoneUrl(base))
    } catch (e) {
      setStatus({ s: 'closed', detail: `Could not reach ${where}: ${String((e as Error).message ?? e)}` })
    } finally {
      setTravelling(null)
    }
  }

  // Names: ask about ids we have not seen named, and about everyone in view every few seconds (players rename).
  useEffect(() => {
    if (!client) return
    const names = shared.current.names
    let n = 0
    const t = setInterval(() => {
      if (n++ % 10 === 0) names.refresh(client.entities().map((e) => e.id))
      void names.poll().then((changed) => {
        if (changed.length === 0) return
        setLines((l) => l.map((x) => (x.from !== undefined && changed.includes(x.from) && x.who !== 'you' && x.who !== 'server' ? { ...x, who: names.name(x.from) } : x)))
      })
    }, 600)
    return () => clearInterval(t)
  }, [client])

  useEffect(() => () => client?.stop(), [client])

  // Pick up wall edits and weather changes made in the console.
  useEffect(() => {
    if (!client) return
    let lastWalls = wallsKey(info)
    let lastWeather = ''
    const t = setInterval(() => {
      fetch(playUrl('/play/info'))
        .then((r) => (r.ok ? r.json() : null))
        .then((fresh: PlayInfo | null) => {
          if (!fresh || fresh.content_hash !== info?.content_hash) return
          if (JSON.stringify(fresh.items) !== JSON.stringify(shared.current.items)) shared.current.items = fresh.items ?? []
          const w = JSON.stringify(fresh.weather ?? null)
          if (w !== lastWeather) {
            lastWeather = w
            shared.current.weather = fresh.weather ?? null
          }
          if (JSON.stringify(fresh.weather_zones ?? []) !== JSON.stringify(shared.current.zones)) shared.current.zones = fresh.weather_zones ?? []
          shared.current.locations = fresh.locations ?? []
          const key = wallsKey(fresh)
          if (key === lastWalls) return
          lastWalls = key
          client.setWalls(fresh)
        })
        .catch(() => undefined)
    }, WORLD_CHECK_MS)
    shared.current.weather = info?.weather ?? null
    shared.current.zones = info?.weather_zones ?? []
    shared.current.locations = info?.locations ?? []
    shared.current.items = info?.items ?? []
    return () => clearInterval(t)
  }, [client, info?.content_hash]) // eslint-disable-line react-hooks/exhaustive-deps

  const recovery = useMemo(() => (identity ? recoveryCode(identity) : null), [identity])
  const playing = !!client && status.s !== 'rejected'

  return (
    <div className="play-skin fixed inset-0 bg-bg text-fg">
      {playing && info && client && (
        <main aria-label="Game view" className="absolute inset-0">
          <Suspense fallback={null}>
            <PlayScene client={client} info={info} shared={shared} dark={dark} />
          </Suspense>
        </main>
      )}
      {!playing && <SignIn info={info} infoError={infoError} rejected={status.s === 'rejected' ? (status.detail ?? 'The server refused the connection.') : null} onEnter={enter} />}
      {playing && info && client && <Hud client={client} info={info} shared={shared} status={status} lines={lines} recovery={recovery} dark={dark} onToggleTheme={toggle} />}
      {travelling && (
        <div role="status" className="absolute inset-0 grid place-items-center bg-bg text-head">
          Travelling to {travelling}
        </div>
      )}
    </div>
  )
}

/** Changes when the walls do: the version for streamed worlds, the boxes themselves for small ones. */
function wallsKey(i: Pick<PlayInfo, 'boxes' | 'walls'> | null | undefined) {
  if (!i) return ''
  // The version covers every box and its class, inline or not.
  return i.walls ? `v${i.walls.ver}` : JSON.stringify(i.boxes)
}
