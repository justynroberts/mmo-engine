import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react'
import { GameClient, type PlayInfo, type Status } from './client'
import Hud, { type Line } from './Hud'
import { newShared } from './shared'
import SignIn from './SignIn'
import { recoveryCode, type ChatMsg } from './protocol'

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

/** The walls can be edited while people play; look for changes now and then so prediction keeps matching the server. */
const WALL_CHECK_MS = 15000

export default function Play() {
  const { dark, toggle } = useTheme()
  const [info, setInfo] = useState<PlayInfo | null>(null)
  const [infoError, setInfoError] = useState<string | null>(null)
  const [client, setClient] = useState<GameClient | null>(null)
  const [identity, setIdentity] = useState<Uint8Array | null>(null)
  const [status, setStatus] = useState<{ s: Status; detail?: string }>({ s: 'connecting' })
  const [lines, setLines] = useState<Line[]>([])
  const shared = useRef(newShared())
  const lineKey = useRef(0)

  useEffect(() => {
    fetch('/play/info')
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`the server said ${r.status}`))))
      .then(setInfo)
      .catch((e) => setInfoError(String(e.message ?? e)))
  }, [])

  const enter = (id: Uint8Array | null) => {
    if (!info) return
    setIdentity(id)
    const url = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`
    const add = (m: ChatMsg, self: number) =>
      setLines((l) => [...l.slice(-30), { key: lineKey.current++, who: m.from === self ? 'you' : `#${m.from & 0xffffff}`, text: m.text, shout: m.channel === 1 }])
    const c: GameClient = new GameClient(info, id, url, {
      onStatus: (s, detail) => setStatus({ s, detail }),
      onChat: (m) => add(m, c.entity),
    })
    setClient(c)
    c.start()
  }

  useEffect(() => () => client?.stop(), [client])

  // Pick up wall edits made in the console while we are playing.
  useEffect(() => {
    if (!client) return
    let last = JSON.stringify(info?.boxes)
    const t = setInterval(() => {
      fetch('/play/info')
        .then((r) => (r.ok ? r.json() : null))
        .then((fresh: PlayInfo | null) => {
          if (!fresh || fresh.content_hash !== info?.content_hash) return
          const key = JSON.stringify(fresh.boxes)
          if (key === last) return
          last = key
          client.setBoxes(fresh.boxes)
          setInfo((p) => (p ? { ...p, boxes: fresh.boxes } : p))
        })
        .catch(() => undefined)
    }, WALL_CHECK_MS)
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
    </div>
  )
}
