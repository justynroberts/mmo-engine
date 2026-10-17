import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Moon, Sun } from 'lucide-react'
import { Lightning, PaperPlaneRight } from '@phosphor-icons/react'
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { GameClient, type PlayInfo, type Status } from './client'
import type { Target } from './PlayScene'
import { identityFromPassphrase, parseRecoveryCode, recoveryCode, type ChatMsg } from './protocol'

const PlayScene = lazy(() => import('./PlayScene'))

type Mode = 'account' | 'recovery' | 'guest'
interface Line {
  key: number
  who: string
  text: string
  shout: boolean
}

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

const remembered = () => {
  try {
    return localStorage.getItem('play-account') ?? ''
  } catch {
    return ''
  }
}

export default function Play() {
  const { dark, toggle } = useTheme()
  const [info, setInfo] = useState<PlayInfo | null>(null)
  const [infoError, setInfoError] = useState<string | null>(null)
  const [mode, setMode] = useState<Mode>('account')
  const [account, setAccount] = useState(remembered)
  const [pass, setPass] = useState('')
  const [code, setCode] = useState('')
  const [busy, setBusy] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)
  const [client, setClient] = useState<GameClient | null>(null)
  const [identity, setIdentity] = useState<Uint8Array | null>(null)
  const [status, setStatus] = useState<{ s: Status; detail?: string }>({ s: 'connecting' })
  const [lines, setLines] = useState<Line[]>([])
  const [hud, setHud] = useState({ hp: 1, hpText: '', visible: 0, rtt: 0, target: '' })
  const [showCode, setShowCode] = useState(false)
  const [copied, setCopied] = useState(false)
  const target = useRef<Target | null>(null)
  const chatBox = useRef<HTMLInputElement>(null)
  const lineKey = useRef(0)

  useEffect(() => {
    fetch('/play/info')
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`the server said ${r.status}`))))
      .then(setInfo)
      .catch((e) => setInfoError(String(e.message ?? e)))
  }, [])

  const begin = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!info) return
    setFormError(null)
    let id: Uint8Array | null = null
    if (mode === 'account') {
      if (!account.trim() || pass.length < 8) {
        setFormError('Pick an account name and a passphrase of at least 8 characters. Longer is better: it is the only thing protecting the character.')
        return
      }
      setBusy(true)
      id = await identityFromPassphrase(account, pass)
      try {
        localStorage.setItem('play-account', account.trim())
      } catch {
        /* fine */
      }
      setPass('')
    } else if (mode === 'recovery') {
      id = parseRecoveryCode(code)
      if (!id) {
        setFormError('That recovery code does not check out. Look for a mistyped character.')
        return
      }
      setBusy(true)
    }
    setIdentity(id)
    const url = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`
    const add = (m: ChatMsg, self: number) =>
      setLines((l) => [...l.slice(-30), { key: lineKey.current++, who: m.from === self ? 'you' : `#${m.from & 0xffffff}`, text: m.text, shout: m.channel === 1 }])
    let c: GameClient
    c = new GameClient(info, id, url, {
      onStatus: (s, detail) => setStatus({ s, detail }),
      onChat: (m) => add(m, c.entity),
    })
    setClient(c)
    c.start()
    setBusy(false)
  }

  useEffect(() => () => client?.stop(), [client])

  // Poll the client for the numbers on screen a few times a second; the scene reads it every frame.
  useEffect(() => {
    if (!client || !info) return
    const hpAttr = info.attrs.find((a) => a.name === 'health') ?? info.replicated.map((i) => info.attrs[i]!)[0]
    const t = setInterval(() => {
      const ents = client.entities()
      const me = ents.find((e) => e.id === client.entity)
      const v = me && hpAttr ? me.attrs[hpAttr.name] : undefined
      const tg = target.current
      setHud({
        hp: v !== undefined && hpAttr ? (v - hpAttr.min) / (hpAttr.max - hpAttr.min) : 1,
        hpText: v !== undefined && hpAttr ? `${hpAttr.name} ${Math.round(v)}` : '',
        visible: Math.max(0, ents.length - 1),
        rtt: client.rtt,
        target: tg ? `${tg.dist.toFixed(0)} away` : '',
      })
    }, 250)
    return () => clearInterval(t)
  }, [client, info])

  const action = info?.actions[0]
  const fire = useCallback(() => {
    if (client && action && target.current) client.act(action.id, target.current.id)
  }, [client, action])

  // Keyboard: W A S D or arrows to move, space to act, Enter for chat.
  useEffect(() => {
    if (!client) return
    const down = new Set<string>()
    const typing = (t: EventTarget | null) => t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')
    const apply = () => {
      const dx = (down.has('d') || down.has('arrowright') ? 1 : 0) - (down.has('a') || down.has('arrowleft') ? 1 : 0)
      const dy = (down.has('s') || down.has('arrowdown') ? 1 : 0) - (down.has('w') || down.has('arrowup') ? 1 : 0)
      client.setMove(dx, dy)
    }
    const onDown = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey) return
      const k = e.key.toLowerCase()
      if (k === 'enter' && !typing(e.target)) {
        e.preventDefault()
        chatBox.current?.focus()
        return
      }
      if (typing(e.target)) return
      if (['w', 'a', 's', 'd', 'arrowup', 'arrowdown', 'arrowleft', 'arrowright'].includes(k)) {
        down.add(k)
        apply()
        e.preventDefault()
      } else if (k === ' ') {
        fire()
        e.preventDefault()
      }
    }
    const onUp = (e: KeyboardEvent) => {
      down.delete(e.key.toLowerCase())
      apply()
    }
    const clear = () => {
      down.clear()
      apply()
    }
    window.addEventListener('keydown', onDown)
    window.addEventListener('keyup', onUp)
    window.addEventListener('blur', clear)
    return () => {
      window.removeEventListener('keydown', onDown)
      window.removeEventListener('keyup', onUp)
      window.removeEventListener('blur', clear)
    }
  }, [client, fire])

  const code68 = useMemo(() => (identity ? recoveryCode(identity) : null), [identity])

  const sendChat = (e: React.FormEvent) => {
    e.preventDefault()
    const el = chatBox.current
    if (!el || !client) return
    const raw = el.value
    const shout = raw.startsWith('!')
    client.say(shout ? raw.slice(1) : raw, shout)
    el.value = ''
    el.blur()
  }

  const hold = (dx: number, dy: number) => ({
    onPointerDown: () => client?.setMove(dx, dy),
    onPointerUp: () => client?.setMove(0, 0),
    onPointerLeave: () => client?.setMove(0, 0),
    onPointerCancel: () => client?.setMove(0, 0),
  })

  const playing = client && status.s !== 'rejected'

  return (
    <div className="fixed inset-0 bg-bg text-fg">
      {playing && info && (
        <main aria-label="Game view" className="absolute inset-0">
          <Suspense fallback={null}>
            <PlayScene client={client} info={info} target={target} />
          </Suspense>
        </main>
      )}

      {!playing && (
        <main className="h-full overflow-y-auto">
          <div className="max-w-[34rem] px-6 pt-16 pb-12">
            <h1 className="text-head m-0 font-medium">mmo-engine</h1>
            <p className="text-fg-3 mt-0">
              {infoError ? `Cannot reach the game: ${infoError}` : info ? 'Sign in to keep your character, or just drop in.' : 'Loading the world…'}
            </p>
            {status.s === 'rejected' && (
              <p role="alert" className="text-accent">
                {status.detail}
              </p>
            )}
            <div role="tablist" aria-label="How to sign in" className="flex gap-4 border-b border-line">
              {(
                [
                  ['account', 'Account'],
                  ['recovery', 'Recovery code'],
                  ['guest', 'Guest'],
                ] as const
              ).map(([m, l]) => (
                <button
                  key={m}
                  role="tab"
                  type="button"
                  aria-selected={mode === m}
                  onClick={() => setMode(m)}
                  className={`tab ${mode === m ? 'text-fg border-b-accent' : ''}`}
                  data-state={mode === m ? 'active' : 'inactive'}
                >
                  {l}
                </button>
              ))}
            </div>
            <form onSubmit={begin} className="pt-4 grid gap-3">
              {mode === 'account' && (
                <>
                  <label className="grid gap-1">
                    <span className="lbl">Account name</span>
                    <input className="field" autoComplete="username" value={account} onChange={(e) => setAccount(e.target.value)} />
                  </label>
                  <label className="grid gap-1">
                    <span className="lbl">Passphrase</span>
                    <input className="field" type="password" autoComplete="current-password" value={pass} onChange={(e) => setPass(e.target.value)} />
                  </label>
                  <p className="text-label text-fg-3 m-0">
                    Your passphrase never leaves this page. It is stretched here into the key that proves who you are, so the same name and passphrase bring the same
                    character back from any device. There is no reset: if you lose it, use a recovery code (shown once you are in).
                  </p>
                </>
              )}
              {mode === 'recovery' && (
                <label className="grid gap-1">
                  <span className="lbl">Recovery code</span>
                  <input className="field" spellCheck={false} autoComplete="off" value={code} onChange={(e) => setCode(e.target.value)} placeholder="xxxxxxxx-xxxxxxxx-…" />
                </label>
              )}
              {mode === 'guest' && <p className="m-0 text-fg-3">Nothing is saved for a guest: when you leave, the character is gone.</p>}
              {formError && (
                <p role="alert" className="m-0 text-accent">
                  {formError}
                </p>
              )}
              <div>
                <button className="btn btn-primary" type="submit" disabled={!info || busy}>
                  {busy ? 'Working…' : 'Enter the world'}
                </button>
              </div>
            </form>
          </div>
        </main>
      )}

      {playing && (
        <aside aria-label="Heads-up display" className="contents">
          <div className="absolute top-3 left-4 right-4 flex items-start justify-between gap-4 pointer-events-none text-[#00ff41] font-mono">
            <div className="text-label leading-5">
              <div role="status">
                {status.s === 'playing' ? 'LIVE' : status.s.toUpperCase()}
                {status.detail ? ` · ${status.detail}` : ''}
              </div>
              <div className="opacity-70">
                {hud.visible} in view · {hud.rtt ? `${Math.round(hud.rtt)} ms` : '… ms'}
              </div>
            </div>
            <div className="w-40">
              <div className="text-label flex justify-between">
                <span>{hud.hpText}</span>
              </div>
              <div className="h-2 border border-[#0a5a1e]" role="img" aria-label={hud.hpText || 'health'}>
                <div className="h-full bg-[#00ff41]" style={{ width: `${Math.max(0, Math.min(1, hud.hp)) * 100}%`, transition: 'width 150ms ease-out' }} />
              </div>
            </div>
          </div>
          <div className="absolute top-3 right-4 mt-12 flex gap-2 text-label">
            <button type="button" className="btn pointer-events-auto bg-black/60 !text-[#00ff41] !border-[#0a5a1e]" onClick={() => setShowCode((v) => !v)} aria-expanded={showCode}>
              {identity ? 'Recovery code' : 'Guest'}
            </button>
            <button type="button" className="btn pointer-events-auto bg-black/60 !text-[#00ff41] !border-[#0a5a1e]" onClick={toggle} aria-label={dark ? 'Switch to light theme' : 'Switch to dark theme'}>
              {dark ? <Sun size={14} /> : <Moon size={14} />}
            </button>
          </div>
          {showCode && (
            <div className="absolute right-4 top-28 w-[22rem] bg-black/85 border border-[#0a5a1e] p-3 text-[#00ff41] text-label">
              {code68 ? (
                <>
                  <p className="m-0 mb-1">Keep this somewhere safe. Anyone with it can play as you, and it is the way back in if you forget your passphrase.</p>
                  <code className="block break-all select-all text-[#d8ffe0]">{code68}</code>
                  <button
                    type="button"
                    className="btn mt-2 !text-[#00ff41] !border-[#0a5a1e]"
                    onClick={() => {
                      void navigator.clipboard?.writeText(code68).then(() => setCopied(true))
                    }}
                  >
                    {copied ? 'Copied' : 'Copy'}
                  </button>
                </>
              ) : (
                <p className="m-0">You are a guest, so there is nothing to recover.</p>
              )}
            </div>
          )}

          <div className="absolute left-4 bottom-4 w-[min(30rem,calc(100%-2rem))] text-[#00ff41] font-mono text-label">
            <ol aria-label="Chat" aria-live="polite" className="list-none p-0 m-0 mb-2 max-h-40 overflow-hidden flex flex-col justify-end">
              {lines.slice(-8).map((l) => (
                <li key={l.key} className={l.shout ? 'text-[#d8ffe0]' : 'opacity-80'}>
                  <span className="opacity-60">{l.shout ? '!' : '>'} {l.who}</span> {l.text}
                </li>
              ))}
            </ol>
            <form onSubmit={sendChat} className="flex items-center gap-2 pointer-events-auto">
              <input
                ref={chatBox}
                aria-label="Chat message"
                maxLength={120}
                placeholder="Enter to chat · start with ! to shout"
                className="field !border-[#0a5a1e] !text-[#00ff41] placeholder:text-[#0f7a2c] bg-black/50 px-2"
              />
              <button type="submit" className="btn !text-[#00ff41] !border-[#0a5a1e] bg-black/50" aria-label="Send">
                <PaperPlaneRight size={14} />
              </button>
            </form>
          </div>

          <div className="absolute right-4 bottom-4 flex items-end gap-4 text-[#00ff41] text-label">
            {action && (
              <button type="button" onClick={fire} className="btn h-10 px-4 bg-black/60 !text-[#00ff41] !border-[#0a5a1e]" aria-label={`${action.name} (space)`}>
                <Lightning size={16} weight="bold" /> {action.name} <span className="opacity-60">{hud.target}</span>
              </button>
            )}
            <div className="grid grid-cols-3 gap-1 [@media(pointer:fine)]:hidden" aria-hidden="true">
              <span />
              <button className="btn h-10 w-10 justify-center bg-black/60 !text-[#00ff41] !border-[#0a5a1e]" {...hold(0, -1)}>
                <ArrowUp size={16} />
              </button>
              <span />
              <button className="btn h-10 w-10 justify-center bg-black/60 !text-[#00ff41] !border-[#0a5a1e]" {...hold(-1, 0)}>
                <ArrowLeft size={16} />
              </button>
              <button className="btn h-10 w-10 justify-center bg-black/60 !text-[#00ff41] !border-[#0a5a1e]" {...hold(0, 1)}>
                <ArrowDown size={16} />
              </button>
              <button className="btn h-10 w-10 justify-center bg-black/60 !text-[#00ff41] !border-[#0a5a1e]" {...hold(1, 0)}>
                <ArrowRight size={16} />
              </button>
            </div>
          </div>
        </aside>
      )}
    </div>
  )
}
