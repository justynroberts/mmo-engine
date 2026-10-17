import { motion, useReducedMotion } from 'framer-motion'
import { Info, Moon, Sun } from 'lucide-react'
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

export function StatusBar({
  s,
  online,
  dark,
  onTheme,
}: {
  s: State | null
  online: boolean
  dark: boolean
  onTheme: () => void
}) {
  return (
    <header className="cell grid gap-x-6 gap-y-2 px-4 py-2 grid-cols-1 lg:grid-cols-[minmax(240px,1.2fr)_2fr_auto] items-start">
      <div className="min-w-0">
        <h1 className="text-head leading-6 tracking-tight">
          mmo-engine<span className="text-fg-3"> / console</span>
        </h1>
        <div className="lbl flex items-center gap-2">
          <span
            className={`inline-block h-2 w-2 ${online ? 'bg-accent' : 'bg-fg-3'}`}
            aria-hidden
          />
          {online ? `live · tick ${s?.tick ?? 0}` : 'server not reachable'}
        </div>
      </div>
      {s ? (
        <div className="grid gap-x-6 gap-y-1 grid-cols-2 md:grid-cols-[1.4fr_1fr_1fr]">
          <div className="col-span-2 md:col-span-1">
            <Ruler s={s} />
          </div>
          <div className="grid grid-cols-3 gap-x-4 col-span-2 md:col-span-2">
            <Cell label="Entities" value={s.live} />
            <Cell label="Players" value={s.clients} />
            <Cell label="Lanes" value={s.perf.lanes} />
            <Cell label="Packets in/s" value={Math.round(s.perf.in_pps)} />
            <Cell label="Packets out/s" value={Math.round(s.perf.out_pps)} />
            <Cell label="Mbit/s out" value={s.perf.mbit.toFixed(1)} />
          </div>
          <div className="grid grid-cols-3 md:grid-cols-6 gap-x-4 col-span-2 md:col-span-3">
            <Cell label="Hello" value={s.net.hellos} />
            <Cell label="Connect" value={s.net.connects} />
            <Cell label="Inputs" value={s.net.inputs} />
            <Cell label="Bad cookie" value={s.net.bad_cookie} warn={s.net.bad_cookie > 0} />
            <Cell label="Auth failed" value={s.net.auth_fail} warn={s.net.auth_fail > 0} />
            <Cell label="Timeouts" value={s.net.timeouts} />
          </div>
        </div>
      ) : (
        <p className="text-fg-3">Waiting for the server at /api/state</p>
      )}
      <div className="justify-self-end flex gap-2">
      <About />
      <button
        type="button"
        className="btn"
        aria-pressed={!dark}
        aria-label={dark ? 'Switch to light theme' : 'Switch to dark theme'}
        onClick={onTheme}
      >
        {dark ? <Sun size={16} strokeWidth={1.5} aria-hidden /> : <Moon size={16} strokeWidth={1.5} aria-hidden />}
        {dark ? 'Light' : 'Dark'}
      </button>
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
