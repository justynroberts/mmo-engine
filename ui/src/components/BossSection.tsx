import { Plus, Trash2 } from 'lucide-react'
import type { Archetype, BossFile, BossMove, BossPhase, GameFile, MoveAt } from '../api'
import { addPhase, bossProblem, MAX_BOSS_ADDS, MAX_BOSS_MOVES, MAX_BOSS_PHASES, newBoss, newMove, removePhase, setPhase } from '../bossEdit'
import { NumField, SelectField, TextField, Toggle } from './Fields'

const AT: { value: MoveAt; label: string }[] = [
  { value: 'tank', label: 'whoever holds it' },
  { value: 'random', label: 'anyone in the fight' },
  { value: 'farthest', label: 'the farthest away' },
  { value: 'self', label: 'round itself' },
]

/** A bot type's boss script: threat, phases by health, telegraphed moves, adds and an enrage timer (`boss.rs`). */
export function BossSection({ game, setGame, arch, a }: { game: GameFile; setGame: (g: GameFile) => void; arch: number; a: Archetype }) {
  const setBoss = (boss: BossFile | undefined) =>
    setGame({ ...game, archetypes: game.archetypes.map((x, k) => (k === arch ? { ...x, boss } : x)) })
  const b = a.boss
  const actions = game.actions.map((x) => ({ value: x.name, label: x.name }))
  const bots = game.archetypes.filter((x) => x.name !== a.name && !x.boss).map((x) => ({ value: x.name, label: x.name }))
  const problem = b ? bossProblem(b, game) : null
  return (
    <section aria-label="Boss fight" className="cell py-2">
      <div className="flex items-end justify-between">
        <h2 className="lbl">Boss fight</h2>
        <Toggle label="A boss" checked={!!b} onChange={(on) => setBoss(on ? newBoss(game, a.attack) : undefined)} />
      </div>
      {!b ? (
        <p className="text-fg-3 py-1">
          A boss goes for whoever has the most threat, marks the ground before its big blows so players can step out, changes how it fights as its
          health falls, and heals fully if everyone runs.
        </p>
      ) : (
        <>
          <div className="grid grid-cols-3 gap-x-2 pt-1">
            <NumField label="Enrage after (s, 0 never)" value={b.enrage_secs ?? 0} onCommit={(n) => setBoss({ ...b, enrage_secs: Math.max(0, Math.min(3600, n)) })} />
            <NumField label="Enraged blows ×" value={b.enrage_might ?? 3} onCommit={(n) => setBoss({ ...b, enrage_might: Math.max(1, Math.min(20, n)) })} />
            <TextField label="Said when it falls" value={b.defeat ?? ''} allowEmpty maxLength={120} onCommit={(t) => setBoss({ ...b, defeat: t || undefined })} />
          </div>
          {problem && (
            <p role="alert" className="m-0 pt-1 text-accent">
              {problem}
            </p>
          )}
          <ol className="list-none p-0 m-0">
            {b.phases.map((p, i) => (
              <li key={i} className="py-2 border-t border-line mt-2">
                <Phase
                  n={i}
                  p={p}
                  actions={actions}
                  bots={bots}
                  set={(np) => setBoss(setPhase(b, i, np))}
                  remove={i > 0 ? () => setBoss(removePhase(b, i)) : undefined}
                  fallback={a.attack ?? game.actions[0]?.name ?? ''}
                />
              </li>
            ))}
          </ol>
          <button type="button" className="btn mt-1" disabled={b.phases.length >= MAX_BOSS_PHASES} onClick={() => setBoss(addPhase(b))}>
            <Plus size={16} strokeWidth={1.5} aria-hidden />
            Add phase
          </button>
        </>
      )}
    </section>
  )
}

type Opt = { value: string; label: string }

function Phase({ n, p, actions, bots, set, remove, fallback }: { n: number; p: BossPhase; actions: Opt[]; bots: Opt[]; set: (p: BossPhase) => void; remove?: () => void; fallback: string }) {
  const setMove = (i: number, m: BossMove) => set({ ...p, moves: p.moves.map((x, k) => (k === i ? m : x)) })
  const adds = p.adds ?? []
  const setAdds = (list: { bot: string; count: number }[]) => {
    const out: BossPhase = { ...p, adds: list }
    if (list.length === 0) delete out.adds
    set(out)
  }
  return (
    <div role="group" aria-label={`Phase ${n + 1}`}>
      <div className="flex items-end gap-2">
        <NumField label={n === 0 ? 'Phase 1 (from the start, %)' : `Phase ${n + 1} below health %`} value={p.below} onCommit={(v) => set({ ...p, below: Math.max(1, Math.min(100, v)) })} className="w-40" />
        <TextField label="Banner as it begins" value={p.say ?? ''} allowEmpty maxLength={120} onCommit={(t) => set({ ...p, say: t || undefined })} className="flex-1" />
        {remove && (
          <button type="button" className="btn px-1" aria-label={`Delete phase ${n + 1}`} onClick={remove}>
            <Trash2 size={16} strokeWidth={1.5} aria-hidden />
          </button>
        )}
      </div>
      {p.moves.map((m, i) => (
        <div key={i} className="grid grid-cols-[1fr_4.5rem_4.5rem_1fr_4rem_auto] gap-x-2 items-end pt-1" role="group" aria-label={`Phase ${n + 1} move ${i + 1}`}>
          <SelectField label="Uses" value={m.action} options={actions} onChange={(v) => setMove(i, { ...m, action: v })} />
          <NumField label="Every s" value={m.every_secs} onCommit={(v) => setMove(i, { ...m, every_secs: Math.max(0.5, Math.min(600, v)) })} />
          <NumField label="Warn s" value={m.warn_secs ?? 0} onCommit={(v) => setMove(i, { ...m, warn_secs: Math.max(0, Math.min(10, v)) || undefined })} />
          <SelectField<MoveAt> label="Aimed at" value={m.at ?? 'tank'} options={AT} onChange={(v) => setMove(i, { ...m, at: v })} />
          <NumField label="Radius" value={m.radius ?? 0} onCommit={(v) => setMove(i, { ...m, radius: Math.max(0, Math.min(64, v)) || undefined })} />
          <button type="button" className="btn px-1" aria-label={`Delete move ${i + 1} of phase ${n + 1}`} onClick={() => set({ ...p, moves: p.moves.filter((_, k) => k !== i) })}>
            <Trash2 size={16} strokeWidth={1.5} aria-hidden />
          </button>
        </div>
      ))}
      {adds.map((x, i) => (
        <div key={i} className="grid grid-cols-[1fr_4.5rem_auto] gap-x-2 items-end pt-1" role="group" aria-label={`Phase ${n + 1} add ${i + 1}`}>
          <SelectField label="Calls in" value={x.bot} options={bots} onChange={(v) => setAdds(adds.map((y, k) => (k === i ? { ...y, bot: v } : y)))} />
          <NumField label="How many" value={x.count} onCommit={(v) => setAdds(adds.map((y, k) => (k === i ? { ...y, count: Math.max(1, Math.min(MAX_BOSS_ADDS, Math.round(v))) } : y)))} />
          <button type="button" className="btn px-1" aria-label={`Delete add ${i + 1} of phase ${n + 1}`} onClick={() => setAdds(adds.filter((_, k) => k !== i))}>
            <Trash2 size={16} strokeWidth={1.5} aria-hidden />
          </button>
        </div>
      ))}
      <div className="flex gap-2 pt-2">
        <button type="button" className="btn" disabled={p.moves.length >= MAX_BOSS_MOVES || !fallback} onClick={() => set({ ...p, moves: [...p.moves, newMove(fallback)] })}>
          <Plus size={16} strokeWidth={1.5} aria-hidden />
          Add move
        </button>
        <button type="button" className="btn" disabled={bots.length === 0} onClick={() => setAdds([...adds, { bot: bots[0]!.value, count: 2 }])}>
          <Plus size={16} strokeWidth={1.5} aria-hidden />
          Add adds
        </button>
      </div>
    </div>
  )
}
