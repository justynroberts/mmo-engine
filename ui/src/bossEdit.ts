// Editing a bot type's boss section (Archetype.boss). Renames and deletes of the actions and bot types it names are
// handled with the rest of the references, in gameEdit.ts (renameAction, removeAction) and directorEdit.ts
// (rewriteRefs).

import type { BossFile, BossMove, BossPhase, GameFile } from './api'

export const MAX_BOSS_PHASES = 8
export const MAX_BOSS_MOVES = 8
export const MAX_BOSS_ADDS = 16
export const MAX_WARN_SECS = 10

/** A first script: one phase, slamming whoever holds it with the bot type's own attack (or the first action). */
export function newBoss(g: GameFile, attack: string | null | undefined): BossFile {
  const action = attack ?? g.actions[0]?.name
  return { phases: [{ below: 100, moves: action ? [newMove(action)] : [] }], enrage_secs: 300 }
}

export function newMove(action: string): BossMove {
  return { action, every_secs: 8, warn_secs: 2, at: 'tank' }
}

/** A phase after the last, at half the health of the one before. */
export function addPhase(b: BossFile): BossFile {
  if (b.phases.length >= MAX_BOSS_PHASES) return b
  const last = b.phases[b.phases.length - 1]?.below ?? 100
  return { ...b, phases: [...b.phases, { below: Math.max(1, Math.round(last / 2)), moves: [] }] }
}

export function setPhase(b: BossFile, i: number, p: BossPhase): BossFile {
  return { ...b, phases: b.phases.map((x, k) => (k === i ? p : x)) }
}

/** The first phase stays (a boss needs one); any other can go. */
export function removePhase(b: BossFile, i: number): BossFile {
  if (i === 0) return b
  return { ...b, phases: b.phases.filter((_, k) => k !== i) }
}

/** What the server will refuse in it, in words, or null. Mirrors proto::game::GameFile::boss_rt. */
export function bossProblem(b: BossFile, g: GameFile): string | null {
  if (b.phases.length === 0 || b.phases.length > MAX_BOSS_PHASES) return `A boss has 1 to ${MAX_BOSS_PHASES} phases.`
  let last = Infinity
  for (const [i, p] of b.phases.entries()) {
    if (!(p.below > 0 && p.below <= 100 && p.below < last)) return `Phase ${i + 1}: its health percent must be above 0, at most 100 and below the phase before.`
    last = p.below
    for (const m of p.moves) {
      if (!g.actions.some((a) => a.name === m.action)) return `Phase ${i + 1}: there is no action called “${m.action}”.`
      if (!(m.every_secs >= 0.5 && m.every_secs <= 600)) return `Phase ${i + 1}: a move is used every 0.5 to 600 seconds.`
      if ((m.warn_secs ?? 0) < 0 || (m.warn_secs ?? 0) > MAX_WARN_SECS) return `Phase ${i + 1}: a warning lasts 0 to ${MAX_WARN_SECS} seconds.`
    }
    for (const x of p.adds ?? []) {
      const t = g.archetypes.find((a) => a.name === x.bot)
      if (!t) return `Phase ${i + 1}: there is no bot type called “${x.bot}”.`
      if (t.boss) return `Phase ${i + 1}: an add cannot be a boss (${x.bot}).`
      if (!(x.count >= 1 && x.count <= MAX_BOSS_ADDS)) return `Phase ${i + 1}: 1 to ${MAX_BOSS_ADDS} of each add.`
    }
  }
  return null
}
