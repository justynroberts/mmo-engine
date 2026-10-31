// Loot tables, recipes, crops and dungeons name items, bot types and places, so renaming one rewrites them and
// deleting one drops whatever can no longer work (a recipe missing an ingredient, a crop with no seed, a dungeon with
// no place), the same way rewriteRefs keeps weather and events honest. A deleted place drops a recipe made there
// rather than widening it to "anywhere".

import type { GameFile } from './api'
import type { RefKind } from './directorEdit'

export function rewriteRealmRefs(g: GameFile, kind: RefKind, from: string, to: string | null): GameFile {
  const next: GameFile = { ...g }
  const swap = (name: string) => (name === from ? to : name)
  if (kind === 'item') {
    if (g.starter) {
      next.starter = g.starter.flatMap((s) => (s.item !== from ? [s] : to === null ? [] : [{ ...s, item: to }]))
      if (next.starter.length === 0) delete next.starter
    }
    if (g.loot) {
      next.loot = g.loot
        .map((t) => ({ ...t, entries: t.entries.flatMap((e) => (e.item !== from ? [e] : to === null ? [] : [{ ...e, item: to }])) }))
        .filter((t) => t.entries.length > 0)
      // A table that emptied out is gone; so is any bot type's pointer at it.
      const kept = new Set(next.loot.map((t) => t.name))
      next.archetypes = next.archetypes.map((a) => {
        if (a.loot === undefined || kept.has(a.loot)) return a
        const out = { ...a }
        delete out.loot
        return out
      })
    }
    if (g.recipes) {
      next.recipes = g.recipes.flatMap((r) => {
        const makes = swap(r.makes)
        const needs = r.needs.map((s) => ({ ...s, item: swap(s.item) }))
        if (makes === null || needs.some((s) => s.item === null)) return []
        return [{ ...r, makes, needs: needs as { item: string; count?: number }[] }]
      })
    }
    if (g.crops) {
      next.crops = g.crops.flatMap((c) => {
        const seed = swap(c.seed)
        const yields = swap(c.yields)
        return seed === null || yields === null ? [] : [{ ...c, seed, yields }]
      })
    }
    if (g.dungeons) {
      next.dungeons = g.dungeons.map((d) => {
        if (d.reward?.item !== from) return d
        const out = { ...d }
        if (to === null) delete out.reward
        else out.reward = { ...d.reward, item: to }
        return out
      })
    }
  }
  if (kind === 'arch' && g.dungeons) {
    next.dungeons = g.dungeons
      .map((d) => ({ ...d, bots: d.bots.flatMap((b) => (b.bot !== from ? [b] : to === null ? [] : [{ ...b, bot: to }])) }))
      .filter((d) => d.bots.length > 0)
  }
  // An achievement for beating a kind of bot follows its name, and goes with it.
  if (kind === 'arch' && g.achievements) {
    next.achievements = g.achievements.flatMap((a) => (a.deed !== `kill:${from}` ? [a] : to === null ? [] : [{ ...a, deed: `kill:${to}` }]))
  }
  if (kind === 'place') {
    if (g.recipes) {
      next.recipes = g.recipes.flatMap((r) => {
        if (r.near !== from) return [r]
        return to === null ? [] : [{ ...r, near: to }]
      })
    }
    if (g.dungeons) {
      next.dungeons = g.dungeons.flatMap((d) => (d.place !== from ? [d] : to === null ? [] : [{ ...d, place: to }]))
    }
  }
  return next
}
