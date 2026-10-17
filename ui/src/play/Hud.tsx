import { ArrowsOutCardinal, Crosshair, MagnifyingGlassMinus, MagnifyingGlassPlus, PaperPlaneRight, Question, SpeakerHigh, SpeakerSlash } from '@phosphor-icons/react'
import { Key, Moon, Sun } from 'lucide-react'
import { useCallback, useEffect, useRef, useState, type MutableRefObject } from 'react'
import type { GameClient, PlayInfo, Status } from './client'
import Bag from './Bag'
import Joystick from './Joystick'
import Minimap from './Minimap'
import { play as sfx, rememberSound, soundWanted } from './sfx'
import { ACT_DROP, ACT_JUMP, ACT_PICKUP, ACT_USE, clampZoom, healthAttr, type Shared } from './shared'

export interface Line {
  key: number
  who: string
  text: string
  shout: boolean
}

interface Props {
  client: GameClient
  info: PlayInfo
  shared: MutableRefObject<Shared>
  status: { s: Status; detail?: string }
  lines: Line[]
  /** The recovery code, or null for a guest. */
  recovery: string | null
  dark: boolean
  onToggleTheme: () => void
}

const TYPING = (t: EventTarget | null) => t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT')

interface Readout {
  hp: number
  hpText: string
  low: boolean
  down: boolean
  visible: number
  rtt: number
  fps: number
  hurt: number
  weather: string
  pos: [number, number] | null
  target: ReturnType<typeof snapshotTarget>
}
function snapshotTarget(s: Shared) {
  const t = s.target
  return t && { id: t.id, dist: Math.round(t.dist), inRange: t.inRange, hp: t.hp, hpText: t.hpText, locked: t.locked }
}

export default function Hud({ client, info, shared, status, lines, recovery, dark, onToggleTheme }: Props) {
  const [r, setR] = useState<Readout>({ hp: 1, hpText: '', low: false, down: false, visible: 0, rtt: 0, fps: 0, hurt: 0, weather: '', pos: null, target: null })
  const [panel, setPanel] = useState<'none' | 'code' | 'help'>('none')
  const [copied, setCopied] = useState(false)
  const [sound, setSound] = useState(soundWanted)
  const soundRef = useRef(sound)
  soundRef.current = sound
  const chatBox = useRef<HTMLInputElement>(null)
  const hurtCount = useRef(0)
  const prevHp = useRef<number | null>(null)
  const wasDown = useRef(false)
  // `G` arms dropping: the next number key drops that slot instead of using it.
  const [dropArmed, setDropArmed] = useState(false)
  const armRef = useRef(0)

  const hpa = healthAttr(info)
  const beep = useCallback((k: Parameters<typeof sfx>[0]) => soundRef.current && sfx(k), [])

  // The numbers on screen refresh ten times a second; the scene reads the same state every frame.
  useEffect(() => {
    const t = setInterval(() => {
      const me = client.last.self
      const v = me && hpa ? me.attrs[hpa.name] : undefined
      const frac = v !== undefined && hpa ? Math.max(0, Math.min(1, (v - hpa.min) / (hpa.max - hpa.min))) : 1
      if (v !== undefined) {
        if (prevHp.current !== null && v < prevHp.current - 0.5) {
          hurtCount.current++
          beep('hurt')
        }
        prevHp.current = v
      }
      const down = v !== undefined && hpa ? v <= hpa.min + 0.01 : false
      if (down && !wasDown.current) beep('down')
      wasDown.current = down
      const next: Readout = {
        hp: frac,
        hpText: v !== undefined && hpa ? `${hpa.name} ${Math.round(v)}` : '',
        low: frac < 0.3,
        down,
        visible: client.last.others.length,
        rtt: Math.round(client.rtt),
        fps: shared.current.fps,
        hurt: hurtCount.current,
        weather: shared.current.weather?.name ?? '',
        pos: me ? [Math.round(me.x * 10) / 10, Math.round(me.y * 10) / 10] : null,
        target: snapshotTarget(shared.current),
      }
      setR((p) => (JSON.stringify(p) === JSON.stringify(next) ? p : next))
    }, 100)
    return () => clearInterval(t)
  }, [client, hpa, shared, beep])

  const fire = useCallback(
    (i: number) => {
      const a = info.actions[i]
      const t = shared.current.target
      if (a && t) {
        client.act(a.id, t.id)
        beep('act')
      }
    },
    [client, info, shared, beep],
  )

  const cycleTarget = useCallback(() => {
    const { self, others } = client.last
    if (!self || others.length === 0) return
    // With nothing locked, T locks what is already shown as the target; after that it steps outward.
    const cur = shared.current.selected
    if (cur === null && shared.current.target) {
      shared.current.selected = shared.current.target.id
      return
    }
    const order = [...others].sort((a, b) => Math.hypot(a.x - self.x, a.y - self.y) - Math.hypot(b.x - self.x, b.y - self.y)).map((e) => e.id)
    const at = cur === null ? -1 : order.indexOf(cur)
    shared.current.selected = order[(at + 1) % order.length] ?? null
  }, [client, shared])

  // Keyboard: W A S D or arrows move, space acts, 1-9 pick an action, T cycles targets, Esc clears, + and - zoom, Enter chats.
  useEffect(() => {
    const down = new Set<string>()
    const move = () => {
      const dx = (down.has('d') || down.has('arrowright') ? 1 : 0) - (down.has('a') || down.has('arrowleft') ? 1 : 0)
      const dy = (down.has('s') || down.has('arrowdown') ? 1 : 0) - (down.has('w') || down.has('arrowup') ? 1 : 0)
      client.setMove(dx, dy)
    }
    const zoom = (f: number) => (shared.current.zoom = clampZoom(shared.current.zoom * f))
    const onDown = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey) return
      const k = e.key.toLowerCase()
      if (k === 'enter' && !TYPING(e.target)) {
        e.preventDefault()
        chatBox.current?.focus()
        return
      }
      if (TYPING(e.target)) return
      if (e.shiftKey && /^Digit[1-8]$/.test(e.code)) {
        const i = Number(e.code.slice(5)) - 1
        client.act(ACT_USE + i, 0)
        beep('act')
        e.preventDefault()
        return
      }
      if (armRef.current > performance.now() && /^[1-8]$/.test(k)) {
        client.act(ACT_DROP + Number(k) - 1, 0)
        armRef.current = 0
        setDropArmed(false)
        beep('act')
        e.preventDefault()
        return
      }
      if (k === 'g') {
        armRef.current = performance.now() + 2500
        setDropArmed(true)
        setTimeout(() => {
          if (armRef.current <= performance.now()) setDropArmed(false)
        }, 2600)
        return
      }
      if (k === 'e') {
        client.act(ACT_PICKUP, 0)
        beep('act')
        return
      }
      if (k === 'j') {
        client.act(ACT_JUMP, 0)
        return
      }
      if (['w', 'a', 's', 'd', 'arrowup', 'arrowdown', 'arrowleft', 'arrowright'].includes(k)) {
        down.add(k)
        move()
        e.preventDefault()
      } else if (k === ' ') {
        fire(0)
        e.preventDefault()
      } else if (/^[1-9]$/.test(k)) {
        fire(Number(k) - 1)
      } else if (k === 't') cycleTarget()
      else if (k === 'escape') shared.current.selected = null
      else if (k === '+' || k === '=') zoom(1 / 1.15)
      else if (k === '-' || k === '_') zoom(1.15)
      else if (k === '?') setPanel((p) => (p === 'help' ? 'none' : 'help'))
    }
    const onUp = (e: KeyboardEvent) => {
      down.delete(e.key.toLowerCase())
      move()
    }
    const clear = () => {
      down.clear()
      move()
    }
    window.addEventListener('keydown', onDown)
    window.addEventListener('keyup', onUp)
    window.addEventListener('blur', clear)
    return () => {
      window.removeEventListener('keydown', onDown)
      window.removeEventListener('keyup', onUp)
      window.removeEventListener('blur', clear)
    }
  }, [client, fire, cycleTarget, shared])

  // A line from someone else makes a small sound when sound is on.
  const seen = useRef(lines.length)
  useEffect(() => {
    if (lines.length > seen.current && lines[lines.length - 1]?.who !== 'you') beep('chat')
    seen.current = lines.length
  }, [lines, beep])

  const sendChat = (e: React.FormEvent) => {
    e.preventDefault()
    const el = chatBox.current
    if (!el) return
    const raw = el.value
    const shout = raw.startsWith('!')
    client.say(shout ? raw.slice(1) : raw, shout)
    el.value = ''
    el.blur()
  }

  const live = status.s === 'playing'
  const t = r.target

  return (
    <aside aria-label="Heads-up display" className="contents">
      <div className="hud absolute inset-0 pointer-events-none text-label" data-testid="hud" data-self-x={r.pos?.[0]} data-self-y={r.pos?.[1]}>
        {r.hurt > 0 && <div key={r.hurt} className="hud-hit absolute inset-0" aria-hidden="true" />}

        <div className="absolute top-3 left-4 w-56 grid gap-2">
          <div className="leading-5">
            <div role="status">
              {live ? 'LIVE' : status.s.toUpperCase()}
              {status.detail ? ` · ${status.detail}` : ''}
            </div>
            <div className="hud-dim">
              {r.visible} in view · {r.rtt ? `${r.rtt} ms` : '… ms'} · {r.fps || '…'} fps
            </div>
            {r.weather && <div>WEATHER · {r.weather}</div>}
          </div>
          {t && (
            <div className="hud-panel p-2 leading-5" aria-label="Target" role="group">
              <div className="flex items-center justify-between">
                <span>
                  <Crosshair size={14} className="inline -mt-0.5 mr-1" aria-hidden />#{t.id & 0xffffff}
                </span>
                <span className={t.inRange ? 'text-[color:var(--hud-accent)]' : 'hud-dim'}>{t.dist} away{t.inRange ? ' · in range' : ''}</span>
              </div>
              {t.hp !== null && (
                <div className="mt-1" role="img" aria-label={t.hpText || 'target health'}>
                  <div className={`hud-bar ${t.hp < 0.3 ? 'low' : ''}`}>
                    <i style={{ width: `${t.hp * 100}%` }} />
                  </div>
                  <div className="hud-dim">{t.hpText}</div>
                </div>
              )}
              <div className="hud-dim">{t.locked ? 'locked · Esc to release' : 'nearest · T or click to lock'}</div>
            </div>
          )}
        </div>

        <div className="absolute top-3 right-4 w-[8.25rem] grid gap-2 justify-items-end pointer-events-none">
          <div className="w-full">
            <div className="flex justify-between">
              <span>{r.hpText || 'health'}</span>
            </div>
            <div className={`hud-bar ${r.low ? 'low' : ''}`} role="img" aria-label={r.hpText || 'health'}>
              <i style={{ width: `${r.hp * 100}%` }} />
            </div>
          </div>
          <div className="flex gap-1 pointer-events-auto flex-wrap justify-end">
            <button type="button" className="hud-btn" aria-label="Recovery code" aria-expanded={panel === 'code'} onClick={() => setPanel((p) => (p === 'code' ? 'none' : 'code'))}>
              <Key size={14} aria-hidden />
              <span className="sr-only">Recovery code</span>
            </button>
            <button type="button" className="hud-btn" aria-label="Controls help" aria-expanded={panel === 'help'} onClick={() => setPanel((p) => (p === 'help' ? 'none' : 'help'))}>
              <Question size={14} aria-hidden />
            </button>
            <button
              type="button"
              className="hud-btn"
              aria-label="Sound effects"
              aria-pressed={sound}
              onClick={() => {
                const on = !sound
                setSound(on)
                rememberSound(on)
                if (on) sfx('chat')
              }}
            >
              {sound ? <SpeakerHigh size={14} aria-hidden /> : <SpeakerSlash size={14} aria-hidden />}
            </button>
            <button type="button" className="hud-btn" onClick={onToggleTheme} aria-label={dark ? 'Switch to light theme' : 'Switch to dark theme'}>
              {dark ? <Sun size={14} aria-hidden /> : <Moon size={14} aria-hidden />}
            </button>
            <button type="button" className="hud-btn" aria-label="Zoom in" onClick={() => (shared.current.zoom = clampZoom(shared.current.zoom / 1.2))}>
              <MagnifyingGlassPlus size={14} aria-hidden />
            </button>
            <button type="button" className="hud-btn" aria-label="Zoom out" onClick={() => (shared.current.zoom = clampZoom(shared.current.zoom * 1.2))}>
              <MagnifyingGlassMinus size={14} aria-hidden />
            </button>
          </div>
          <Minimap client={client} info={info} shared={shared} dark={dark} />
        </div>

        {panel === 'code' && (
          <div className="hud-panel absolute right-[10.5rem] top-3 w-[min(22rem,calc(100%-12rem))] p-3 pointer-events-auto">
            {recovery ? (
              <>
                <p className="m-0 mb-1">Keep this somewhere safe. Anyone with it can play as you, and it is the way back in if you forget your passphrase.</p>
                <code className="block break-all select-all">{recovery}</code>
                <button
                  type="button"
                  className="hud-btn mt-2"
                  onClick={() => {
                    void navigator.clipboard?.writeText(recovery).then(() => setCopied(true))
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
        {panel === 'help' && (
          <div className="hud-panel absolute right-[10.5rem] top-3 w-[min(22rem,calc(100%-12rem))] p-3 pointer-events-auto" role="region" aria-label="Controls">
            <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3">
              {[
                ['W A S D / arrows', 'move'],
                ['Space', info.actions[0] ? info.actions[0].name : 'act'],
                ...(info.actions.length > 1 ? [['1 to ' + Math.min(9, info.actions.length), 'pick an action']] : []),
                ['Shift + 1 to 8', 'use a bag item'],
                ['G, then 1 to 8', 'drop a bag item'],
                ['E', 'pick up what is at your feet'],
                ['J', 'jump (attacks miss you in the air)'],
                ['Click / tap', 'target a character'],
                ['T', 'next target'],
                ['Esc', 'release the target'],
                ['Enter', 'chat (start with ! to shout)'],
                ['Wheel, + and −', 'zoom'],
                ['?', 'this panel'],
              ].map(([k, v]) => (
                <div key={k} className="contents">
                  <dt>
                    <kbd className="font-[inherit]">{k}</kbd>
                  </dt>
                  <dd className="m-0 hud-dim">{v}</dd>
                </div>
              ))}
            </dl>
          </div>
        )}

        {(!live || r.down) && (
          <div className="absolute inset-0 grid place-items-center">
            <div role="status" className="hud-panel px-6 py-4 text-center">
              {!live ? (
                <>
                  <div className="text-head">{status.s === 'connecting' ? 'RECONNECTING' : 'DISCONNECTED'}</div>
                  <div className="hud-dim">{status.detail ?? 'hold on'}</div>
                </>
              ) : (
                <>
                  <div className="text-head" style={{ color: 'var(--hud-warn)' }}>
                    SIGNAL LOST
                  </div>
                  <div className="hud-dim">waiting to respawn</div>
                </>
              )}
            </div>
          </div>
        )}

        <div className="hud-chat absolute left-4 bottom-4 w-[min(30rem,calc(100%-2rem))]">
          <ol aria-label="Chat" aria-live="polite" className="list-none p-0 m-0 mb-2 max-h-40 overflow-hidden flex flex-col justify-end">
            {lines.slice(-8).map((l) => (
              <li key={l.key} style={{ textShadow: '0 0 6px var(--hud-bg)' }}>
                <span className="hud-dim">
                  {l.shout ? '!' : '>'} {l.who}
                </span>{' '}
                {l.text}
              </li>
            ))}
          </ol>
          <form onSubmit={sendChat} className="flex items-center gap-2 pointer-events-auto">
            <input ref={chatBox} aria-label="Chat message" maxLength={120} placeholder="Enter to chat · start with ! to shout" className="hud-field" />
            <button type="submit" className="hud-btn" aria-label="Send">
              <PaperPlaneRight size={14} aria-hidden />
            </button>
          </form>
        </div>

        <Bag client={client} shared={shared} onAct={() => beep('act')} dropArmed={dropArmed} />

        <div className="absolute right-4 bottom-4 flex items-end gap-3">
          <div className="flex flex-col gap-1 pointer-events-auto">
            {info.actions.map((a, i) => (
              <button key={a.id} type="button" onClick={() => fire(i)} className="hud-btn h-10 px-4" aria-label={`${a.name}${i === 0 ? ' (space)' : ` (${i + 1})`}`}>
                <ArrowsOutCardinal size={16} aria-hidden /> {a.name}
                <span className="hud-dim">{i === 0 ? '␣' : i + 1}</span>
              </button>
            ))}
          </div>
        </div>

        <div className="absolute left-4 bottom-4 pointer-events-auto [@media(pointer:fine)]:hidden">
          <Joystick onMove={(x, y) => client.setMove(x, y)} />
        </div>
      </div>
    </aside>
  )
}
