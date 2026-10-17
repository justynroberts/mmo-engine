import { Check, Copy, Crosshair, Gift, Skull } from 'lucide-react'
import { useEffect, useState } from 'react'
import { api, KIND_BOT, KIND_DRIVEN, KIND_PLAYER, type GameFile, type Info, type State } from '../api'
import { BEHAVIOUR_ICON } from './Editors'
import { Empty } from './Fields'

export function EntityPanel({
  s,
  game,
  selected,
  possessed,
  follow,
  onFollow,
  onSelect,
  onPossess,
  onKill,
  actionName,
}: {
  s: State | null
  game: GameFile | null
  selected: number | null
  possessed: number | null
  follow: boolean
  onFollow: (v: boolean) => void
  onSelect: (id: number | null) => void
  onPossess: (id: number | null) => void
  onKill: (id: number) => void
  actionName?: string | undefined
}) {
  if (!s) return <Empty>No data yet.</Empty>
  const row = s.e.find((r) => r[0] === selected)
  const arch = row && (row[3] ?? -1) >= 0 ? game?.archetypes[row[3] ?? 0] : undefined
  const kind = row?.[4]
  const sorted = [...s.e].sort((a, b) => (b[4] ?? 0) - (a[4] ?? 0)).slice(0, 120)
  return (
    <div>
      <section className="cell py-2" aria-label="Selected entity">
        {row ? (
          <>
            <div className="flex items-baseline justify-between">
              <h2 className="text-head leading-6">
                #{(row[0] ?? 0) & 0xffffff}
                <span className="text-fg-3"> gen {(row[0] ?? 0) >>> 24}</span>
              </h2>
              <span className="lbl flex items-center gap-1">
                {arch ? BEHAVIOUR_ICON[arch.behaviour] : null}
                {kind === KIND_PLAYER ? 'player' : kind === KIND_DRIVEN ? 'possessed' : arch ? arch.name : 'plain npc'}
              </span>
            </div>
            <p className="text-fg-2">
              x {(row[1] ?? 0).toFixed(1)} · y {(row[2] ?? 0).toFixed(1)}
            </p>
            <dl className="pt-1">
              {s.attrs.map((name, i) => {
                const def = game?.attrs.find((a) => a.name === name)
                const v = row[5 + i] ?? 0
                const t = def ? (v - def.min) / (def.max - def.min || 1) : 0
                return (
                  <div key={name} className="grid grid-cols-[6rem_1fr_4rem] items-center gap-2">
                    <dt className="lbl truncate">{name}</dt>
                    <dd className="m-0 h-[6px] bg-line relative" aria-hidden>
                      <div className="absolute inset-y-0 left-0 bg-fg" style={{ width: `${Math.max(0, Math.min(1, t)) * 100}%` }} />
                    </dd>
                    <dd className="m-0 text-right">{v.toFixed(1)}</dd>
                  </div>
                )
              })}
            </dl>
            <div className="flex flex-wrap gap-2 pt-2">
              {kind !== KIND_PLAYER && (
                <button
                  type="button"
                  className={`btn ${possessed === row[0] ? 'btn-primary' : ''}`}
                  aria-pressed={possessed === row[0]}
                  onClick={() => onPossess(possessed === row[0] ? null : (row[0] ?? null))}
                >
                  <Crosshair size={16} strokeWidth={1.5} aria-hidden />
                  {possessed === row[0] ? 'Release' : 'Possess'}
                </button>
              )}
              <label className="btn">
                <input type="checkbox" checked={follow} onChange={(e) => onFollow(e.target.checked)} className="accent-[var(--color-accent)]" />
                Follow
              </label>
              {kind !== KIND_PLAYER && (
                <button type="button" className="btn" onClick={() => onKill(row[0] ?? 0)}>
                  <Skull size={16} strokeWidth={1.5} aria-hidden />
                  Remove
                </button>
              )}
            </div>
            {kind === KIND_PLAYER && (game?.items ?? []).length > 0 && <Bag id={row[0] ?? 0} items={(game?.items ?? []).map((i) => i.name)} />}
            {possessed === row[0] && (
              <p className="text-fg-3 pt-1">
                WASD or arrows to walk{actionName ? `, space to use ${actionName} on the nearest entity in range` : ''}. Keys are
                ignored while a text field has focus.
              </p>
            )}
          </>
        ) : (
          <Empty>Click an entity on the map, or pick one below.</Empty>
        )}
      </section>
      <section aria-label="All entities">
        <h2 className="lbl pt-2">Entities ({s.live})</h2>
        <ul className="m-0 p-0 list-none">
          {sorted.map((r) => {
            const a = (r[3] ?? -1) >= 0 ? game?.archetypes[r[3] ?? 0] : undefined
            const k = r[4]
            return (
              <li key={r[0]}>
                <button
                  type="button"
                  onClick={() => onSelect(r[0] ?? null)}
                  aria-current={r[0] === selected}
                  className={`w-full text-left h-6 px-0 bg-transparent border-0 border-b border-line cursor-pointer flex gap-2 ${r[0] === selected ? 'text-accent' : 'text-fg-2 hover:text-fg'}`}
                >
                  <span className="w-14">#{(r[0] ?? 0) & 0xffffff}</span>
                  <span className="flex-1 truncate">{k === KIND_PLAYER ? 'player' : k === KIND_DRIVEN ? `${a?.name ?? 'npc'} (possessed)` : (a?.name ?? 'plain npc')}</span>
                  <span>
                    {(r[1] ?? 0).toFixed(0)},{(r[2] ?? 0).toFixed(0)}
                  </span>
                </button>
              </li>
            )
          })}
        </ul>
        {s.live > sorted.length && <p className="text-fg-3">+{s.live - sorted.length} more, use the map.</p>}
      </section>
      <span hidden>{KIND_BOT}</span>
    </div>
  )
}

function CopyButton({ text, label }: { text: string; label: string }) {
  const [done, setDone] = useState(false)
  return (
    <button
      type="button"
      className="btn"
      aria-label={label}
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => {
          setDone(true)
          setTimeout(() => setDone(false), 1200)
        })
      }}
    >
      {done ? <Check size={16} strokeWidth={1.5} aria-hidden /> : <Copy size={16} strokeWidth={1.5} aria-hidden />}
      {done ? 'Copied' : 'Copy'}
    </button>
  )
}

export function ConnectPanel({ info }: { info: Info | null }) {
  if (!info) return <Empty>Waiting for the server.</Empty>
  const port = info.udp_port
  const cmd = `cargo run --release -p bots -- --server 127.0.0.1:${port} --server-key ${info.server_key} --bots 100`
  return (
    <div>
      <section className="cell py-2">
        <h2 className="lbl">Server public key</h2>
        <p className="break-all">{info.server_key}</p>
        <p className="text-fg-3 pb-1">
          Clients pin this. A client that is given it refuses any server that cannot prove it holds the matching private key
          (<code>server.key</code> next to the server).
        </p>
        <CopyButton text={info.server_key} label="Copy server public key" />
      </section>
      <section className="cell py-2">
        <h2 className="lbl">Load test with real encrypted clients</h2>
        <pre className="m-0 whitespace-pre-wrap break-all text-fg-2">{cmd}</pre>
        <div className="pt-1">
          <CopyButton text={cmd} label="Copy bot command" />
        </div>
      </section>
      <section className="py-2">
        <dl className="grid grid-cols-2 gap-x-4">
          {(
            [
              ['UDP port', port],
              ['Tick rate', `${info.hz} Hz`],
              ['World', `${info.world_size} × ${info.world_size}`],
              ['Seed', info.seed],
              ['Content hash', info.content_hash],
              ['Protocol', info.proto],
              ['Capacity', info.capacity],
              ['Snapshot lanes', info.snapshot_lanes],
            ] as const
          ).map(([k, v]) => (
            <div key={k} className="cell flex justify-between">
              <dt className="lbl">{k}</dt>
              <dd className="m-0">{v}</dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  )
}

export function RawPanel({ text, onText, error }: { text: string; onText: (s: string) => void; error: string | null }) {
  return (
    <div className="py-2">
      <label className="block">
        <span className="lbl block">game.json (edits here update the draft when the JSON is valid)</span>
        <textarea
          className="w-full h-[28rem] bg-transparent border border-line rounded-sm p-2 text-fg resize-y font-mono"
          spellCheck={false}
          value={text}
          onChange={(e) => onText(e.target.value)}
          aria-invalid={error !== null}
        />
      </label>
      {error && (
        <p role="alert" className="text-accent">
          {error}
        </p>
      )}
    </div>
  )
}

/** A player's bag as the server holds it, and a way to hand them something. */
function Bag({ id, items }: { id: number; items: string[] }) {
  const [bag, setBag] = useState<{ item: string; count: number }[]>([])
  const [pick, setPick] = useState('')
  const [n, setN] = useState(1)
  const [note, setNote] = useState<string | null>(null)
  const [tick, setTick] = useState(0)
  useEffect(() => {
    let live = true
    const load = () =>
      void api<{ bag?: { item: string; count: number }[] }>(`/api/inventory?id=${id}`).then((r) => {
        if (live && r.status === 200) setBag(r.body.bag ?? [])
      })
    load()
    const t = setInterval(load, 1500)
    return () => {
      live = false
      clearInterval(t)
    }
  }, [id, tick])
  const item = items.includes(pick) ? pick : (items[0] ?? '')
  return (
    <div className="pt-2" role="group" aria-label="Bag">
      <p className="lbl">Carrying</p>
      <p className="m-0 text-fg-2">{bag.length ? bag.map((b) => `${b.item} ×${b.count}`).join(', ') : 'nothing'}</p>
      <div className="flex items-end gap-2 pt-1">
        <label className="block flex-1">
          <span className="lbl block">Give</span>
          <select className="field cursor-pointer" value={item} onChange={(e) => setPick(e.target.value)}>
            {items.map((x) => (
              <option key={x} value={x} className="bg-bg">
                {x}
              </option>
            ))}
          </select>
        </label>
        <label className="block w-16">
          <span className="lbl block">How many</span>
          <input className="field" type="number" min={1} max={99} value={n} onChange={(e) => setN(Math.max(1, Math.min(99, Number(e.target.value) || 1)))} />
        </label>
        <button
          type="button"
          className="btn"
          onClick={() =>
            void api<{ given?: number; dropped?: number; error?: string }>('/api/give', 'POST', { id, item, count: n }).then((r) => {
              setNote(r.status === 200 ? `Gave ${r.body.given}${r.body.dropped ? `, ${r.body.dropped} did not fit and fell at their feet` : ''}.` : (r.body.error ?? 'refused'))
              setTick((t) => t + 1)
            })
          }
        >
          <Gift size={16} strokeWidth={1.5} aria-hidden />
          Give
        </button>
      </div>
      {note && (
        <p role="status" className="m-0 pt-1 text-fg-3">
          {note}
        </p>
      )}
    </div>
  )
}
