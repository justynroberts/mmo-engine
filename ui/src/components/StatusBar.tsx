import { motion, useReducedMotion } from 'framer-motion'
import { Info } from 'lucide-react'
import { useRef } from 'react'
import type { State } from '../api'

function Cell({ label, value, warn }: { label: string; value: string | number; warn?: boolean }) {
  return (
    <div className="min-w-0">
      <div className="lbl truncate">{label}</div>
      <div className={`truncate ${warn ? 'text-accent' : ''}`}>{value}</div>
    </div>
  )
}

/** The 50 ms tick budget as a ruler: phases are segments, p99 is the marker. */
function Ruler({ s }: { s: State }) {
  const reduce = useReducedMotion()
  const budget = 1000 / s.hz
  const p = s.perf
  const seg = [
    { k: 'recv', ms: p.recv_ms, c: 'var(--color-fg-3)' },
    { k: 'sim', ms: p.sim_ms, c: 'var(--color-fg-2)' },
    { k: 'snap+send', ms: p.snap_ms, c: 'var(--color-fg)' },
  ]
  const pct = (ms: number) => `${Math.min(100, (ms / budget) * 100)}%`
  const over = p.tick_p99_ms > budget
  return (
    <div role="img" aria-label={`Tick budget ${budget} milliseconds. Average ${p.tick_avg_ms.toFixed(1)}, 99th percentile ${p.tick_p99_ms.toFixed(1)}`}>
      <div className="lbl">Tick budget {budget.toFixed(0)} ms</div>
      <div className="relative h-2 border border-line mt-1">
        <div className="absolute inset-0 flex">
          {seg.map((x) => (
            <motion.div
              key={x.k}
              title={`${x.k} ${x.ms.toFixed(2)} ms`}
              style={{ background: x.c, height: '100%' }}
              initial={false}
              animate={{ width: pct(x.ms) }}
              transition={{ duration: reduce ? 0 : 0.15, ease: 'easeOut' }}
            />
          ))}
        </div>
        <motion.div
          className="absolute top-[-3px] bottom-[-3px] w-[2px] bg-accent"
          initial={false}
          animate={{ left: pct(p.tick_p99_ms) }}
          transition={{ duration: reduce ? 0 : 0.15, ease: 'easeOut' }}
        />
      </div>
      <div className="flex flex-wrap gap-x-4 text-label text-fg-3 pt-1">
        {seg.map((x) => (
          <span key={x.k} className="flex items-center gap-1">
            <i className="inline-block h-2 w-2" style={{ background: x.c }} aria-hidden />
            {x.k} {x.ms.toFixed(1)}
          </span>
        ))}
        <span className={over ? 'text-accent' : 'text-fg-2'}>
          avg {p.tick_avg_ms.toFixed(1)} · p99 {p.tick_p99_ms.toFixed(1)} ms
        </span>
      </div>
    </div>
  )
}

/** The server's own numbers in full: the tick budget and every network counter (the Server page). */
export function NetworkPanel({ s }: { s: State | null }) {
  if (!s) return <p className="text-fg-3">Waiting for the server at /api/state.</p>
  return (
    <div className="grid gap-4">
      <Ruler s={s} />
      <div className="grid grid-cols-3 gap-x-6 gap-y-3">
        <Cell label="Entities" value={s.live} />
        <Cell label="Players connected" value={s.clients} />
        <Cell label="Snapshot lanes" value={s.perf.lanes} />
        <Cell label="Packets in / s" value={Math.round(s.perf.in_pps)} />
        <Cell label="Packets out / s" value={Math.round(s.perf.out_pps)} />
        <Cell label="Mbit/s out" value={s.perf.mbit.toFixed(1)} />
        <Cell label="Hellos" value={s.net.hellos} />
        <Cell label="Connects" value={s.net.connects} />
        <Cell label="Inputs" value={s.net.inputs} />
        <Cell label="Bad cookies" value={s.net.bad_cookie} warn={s.net.bad_cookie > 0} />
        <Cell label="Failed authentication" value={s.net.auth_fail} warn={s.net.auth_fail > 0} />
        <Cell label="Timeouts" value={s.net.timeouts} />
      </div>
      {s.zone && <ZonePanel s={s} />}
    </div>
  )
}

/** Where this server sits among the zones: its name and neighbours, and the players passed between them. */
function ZonePanel({ s }: { s: State }) {
  const z = s.zone!
  const name = (i: number) => z.names[i] || `zone ${i}`
  const others = Array.from({ length: z.count }, (_, i) => i).filter((i) => i !== z.index).map(name)
  return (
    <div className="grid gap-2 border-t border-line pt-3">
      <div className="lbl">
        {z.lines ? 'Zone lines' : 'Zone strips'}: this is {name(z.index)} ({z.index + 1} of {z.count})
        {others.length > 0 && <span className="text-fg-3"> · others: {others.join(', ')}</span>}
      </div>
      <div className="grid grid-cols-3 gap-x-6 gap-y-3">
        {!z.lines && <Cell label="Owns x" value={`${z.lo} to ${z.hi}`} />}
        <Cell label="Handed on" value={z.out} />
        <Cell label="Arrived" value={z.in} />
        {z.lines && <Cell label="Sent on at sign-in" value={s.net.sent_on ?? 0} />}
        <Cell label="Failed handoffs" value={z.failed} warn={z.failed > 0} />
        <Cell label="Refused" value={z.rejected} warn={z.rejected > 0} />
        {!z.lines && <Cell label="Border ghosts" value={z.ghosts} />}
      </div>
    </div>
  )
}

/** One headline number in the header: a word and a value, no jargon. */
function Stat({ label, value, warn }: { label: string; value: string; warn?: boolean }) {
  return (
    <div className="min-w-0">
      <div className="text-label text-fg-3 truncate">{label}</div>
      <div className={`font-semibold truncate ${warn ? 'text-accent' : 'text-fg'}`}>{value}</div>
    </div>
  )
}

/** The top of every page: what this is and whether the server is up, the three numbers that matter, and the
 * Apply / Revert for changes waiting to go to the server (they are made on any page, so they live here). */
export function Header({
  s,
  online,
  dirty,
  restarts,
  applying,
  onApply,
  onRevert,
  result,
}: {
  s: State | null
  online: boolean
  dirty: boolean
  /** The waiting changes alter the wire format, so applying them restarts the world. */
  restarts: boolean
  applying: boolean
  onApply: () => void
  onRevert: () => void
  result: { ok: boolean; text: string } | null
}) {
  const budget = s ? 1000 / s.hz : 50
  const p99 = s?.perf.tick_p99_ms ?? 0
  const bots = s ? Math.max(0, s.live - s.clients) : 0
  return (
    <header className="flex flex-wrap items-center gap-x-8 gap-y-2 px-5 py-3 border-b border-line">
      <div className="min-w-0">
        <h1 className="text-head font-semibold m-0">
          mmo-engine <span className="text-fg-3 font-normal">admin</span>
        </h1>
        <div className="flex items-center gap-2 text-label text-fg-3">
          <span className={`inline-block h-2 w-2 rounded-full ${online ? 'bg-accent' : 'bg-fg-3'}`} aria-hidden />
          {online ? 'Server running' : 'Server not reachable'}
        </div>
      </div>
      {s && (
        <div className="flex flex-wrap gap-x-8 gap-y-2 min-w-0 max-w-full">
          <Stat label="Players online" value={String(s.clients)} />
          <Stat label="Bots and others" value={bots.toLocaleString()} />
          <Stat
            label="Server load"
            value={`${Math.round((s.perf.tick_avg_ms / budget) * 100)}% busy${p99 > budget ? `, spikes to ${p99.toFixed(0)} ms` : ''}`}
            warn={s.perf.tick_avg_ms > budget * 0.7}
          />
        </div>
      )}
      <div className="ml-auto flex items-center gap-3" aria-label="Changes">
        <div aria-live="polite" className="text-right max-w-[34ch]">
          {result && !result.ok ? (
            <span role="alert" className="text-accent">
              {result.text}
            </span>
          ) : dirty ? (
            <span className={restarts ? 'text-accent' : 'text-fg-2'}>{restarts ? 'Unsaved changes: applying restarts the world' : 'Unsaved changes: they apply live'}</span>
          ) : result ? (
            <span role={result.ok ? 'status' : 'alert'} className={result.ok ? 'text-fg-3' : 'text-accent'}>
              {result.text}
            </span>
          ) : (
            <span className="text-fg-3">Everything is saved</span>
          )}
        </div>
        <button type="button" className="btn" disabled={!dirty || applying} onClick={onRevert}>
          Revert
        </button>
        <button type="button" className={`btn ${dirty ? 'btn-primary' : ''}`} disabled={!dirty || applying} onClick={onApply}>
          {applying ? 'Applying…' : 'Apply'}
        </button>
        <About />
      </div>
    </header>
  )
}

/** The house credit: a small button that opens a dialog. Escape, the backdrop and Close all shut it. */
function About() {
  const dlg = useRef<HTMLDialogElement>(null)
  return (
    <>
      <button type="button" className="btn" aria-label="About this app" aria-haspopup="dialog" onClick={() => dlg.current?.showModal()}>
        <Info size={16} strokeWidth={1.5} aria-hidden />
      </button>
      <dialog
        ref={dlg}
        aria-labelledby="about-title"
        className="m-auto border border-line rounded-sm bg-bg text-fg p-4 backdrop:bg-black/60"
        onClick={(e) => {
          if (e.target === dlg.current) dlg.current?.close()
        }}
      >
        <h2 id="about-title" className="text-head leading-6 m-0">mmo-engine console</h2>
        <p className="lbl m-0">Version 0.1.0</p>
        <p className="m-0 mt-2">
          Made by{' '}
          <a href="https://fintonlabs.com" target="_blank" rel="noopener" className="text-accent">
            FintonLabs
          </a>
        </p>
        <button type="button" className="btn mt-3" onClick={() => dlg.current?.close()}>
          Close
        </button>
      </dialog>
    </>
  )
}
