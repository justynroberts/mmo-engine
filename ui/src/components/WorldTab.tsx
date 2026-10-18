import { ArrowsOutCardinal, Eraser, Plus, Shuffle, X } from '@phosphor-icons/react'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { defaultRecipe, parseWorld, type Recipe, type WorldData, type WorldEdit } from '../world'
import { Range } from './Builder'
import { NumField, SelectField, Toggle } from './Fields'
import type { ShapeOverlay } from './WorldView'

// The World tab: make the world from a recipe (seed, size, how much of each thing, hand edits), see it on the
// map as a preview, and put it live. The server runs the generator (worldgen); this tab never decides anything
// about the land itself.

const ICON = { size: 16, weight: 'regular' as const }
const ADDABLE = ['house', 'keep', 'tower', 'castle', 'wall', 'crag', 'lake', 'square'] as const
const SIZES = [1024, 2048, 4096, 8192] as const
const DIALS: readonly [keyof Recipe & ('villages' | 'forts' | 'crags' | 'lakes' | 'forest' | 'hills'), string][] = [
  ['villages', 'Villages'],
  ['forts', 'Forts'],
  ['crags', 'Crags'],
  ['lakes', 'Lakes'],
  ['forest', 'Forest'],
  ['hills', 'Hills'],
]

export type ShapeTool = ShapeOverlay['tool']

export interface Shaper {
  /** The recipe being edited (the live world's until something changes). */
  recipe: Recipe
  /** True once the recipe differs from the live world's. */
  dirty: boolean
  set: (r: Recipe) => void
  preview: WorldData | null
  busy: boolean
  error: string | null
  note: string | null
  tool: ShapeTool
  setTool: (t: ShapeTool) => void
  addType: string
  setAddType: (t: string) => void
  apply: () => void
  discard: () => void
  /** The map overlay while the World tab is open. */
  overlay: ShapeOverlay
}

/** World tab state, kept in the app so the map can show the preview and take edits. */
export function useShaper(land: WorldData | null, size: number, seed: number): Shaper {
  const [recipe, setRecipe] = useState<Recipe | null>(null)
  const [preview, setPreview] = useState<WorldData | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [note, setNote] = useState<string | null>(null)
  const [tool, setTool] = useState<ShapeTool>('pan')
  const [addType, setAddType] = useState<string>('house')
  const asked = useRef(0)
  const cur = recipe ?? land?.recipe ?? defaultRecipe(size, seed)

  // Every change makes a fresh preview (a 2048 world takes the generator a fraction of a second).
  useEffect(() => {
    if (!recipe) {
      setPreview(null)
      setError(null)
      return
    }
    const n = ++asked.current
    const t = setTimeout(() => {
      setBusy(true)
      fetch('/api/world/generate', { method: 'POST', headers: { 'X-Admin': '1', 'Content-Type': 'application/json' }, body: JSON.stringify(recipe) })
        .then(async (r) => {
          const text = await r.text()
          if (n !== asked.current) return
          if (!r.ok) {
            let msg = text
            try {
              msg = (JSON.parse(text) as { error?: string }).error ?? text
            } catch {
              // not JSON: show it as it came
            }
            setError(msg.slice(0, 300))
            return
          }
          setError(null)
          setPreview(parseWorld(text))
        })
        .catch((e: unknown) => n === asked.current && setError(String(e)))
        .finally(() => n === asked.current && setBusy(false))
    }, 250)
    return () => clearTimeout(t)
  }, [recipe])

  const set = (r: Recipe) => {
    setNote(null)
    setRecipe(r)
  }
  const apply = () => {
    setBusy(true)
    fetch('/api/world/apply', { method: 'POST', headers: { 'X-Admin': '1' } })
      .then(async (r) => {
        const body = (await r.json()) as { ok?: boolean; error?: string; restart?: boolean; note?: string; moved_entities?: number }
        if (!r.ok || !body.ok) {
          setError(body.error ?? `HTTP ${r.status}`)
          return
        }
        setNote(body.restart ? (body.note ?? 'Saved; restart the server to use it.') : `Live. ${body.moved_entities ?? 0} entities moved out of new walls.`)
        // The live world now has this recipe; the map switches back to it when the new version arrives.
        setRecipe(null)
      })
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setBusy(false))
  }
  const discard = () => {
    setRecipe(null)
    setNote(null)
  }
  const round = (v: number) => Math.round(v * 10) / 10
  const overlay: ShapeOverlay = {
    tool,
    edits: cur.edits,
    previewing: recipe !== null,
    onRemove: (x, y) => set({ ...cur, edits: [...cur.edits, { remove: [round(x), round(y)] }] }),
    onAdd: (x0, y0, x1, y1) => {
      const b: [number, number, number, number] = [Math.round(Math.min(x0, x1)), Math.round(Math.min(y0, y1)), Math.round(Math.max(x0, x1)), Math.round(Math.max(y0, y1))]
      if (b[2] - b[0] < 2 || b[3] - b[1] < 2) return
      set({ ...cur, edits: [...cur.edits, { add: { t: addType, b } }] })
    },
  }
  return { recipe: cur, dirty: recipe !== null, set, preview, busy, error, note, tool, setTool, addType, setAddType, apply, discard, overlay }
}

function describe(e: WorldEdit): string {
  if ('remove' in e) return `Remove at ${Math.round(e.remove[0])}, ${Math.round(e.remove[1])}`
  const [x0, y0, x1, y1] = e.add.b
  return `Add ${e.add.t} at ${x0}, ${y0} (${x1 - x0} × ${y1 - y0})`
}

const TOOLS: readonly { tool: ShapeTool; label: string; icon: ReactNode; hint: string }[] = [
  { tool: 'pan', label: 'Look', icon: <ArrowsOutCardinal {...ICON} aria-hidden />, hint: 'Drag to pan, scroll to zoom.' },
  { tool: 'remove', label: 'Remove', icon: <Eraser {...ICON} aria-hidden />, hint: 'Click a building, crag or lake to take it away. On a city’s open ground it takes the whole city.' },
  { tool: 'add', label: 'Add', icon: <Plus {...ICON} aria-hidden />, hint: 'Drag a rectangle to put one there. It clears whatever it covers, and the land is levelled round it.' },
]

export function WorldTab({ s, land, liveSize, blocks }: { s: Shaper; land: WorldData | null; liveSize: number; blocks: number }) {
  const r = s.recipe
  const [confirmClear, setConfirmClear] = useState(false)
  const [cleared, setCleared] = useState<string | null>(null)
  const clearBlocks = () => {
    setConfirmClear(false)
    fetch('/api/blocks/clear', { method: 'POST', headers: { 'X-Admin': '1' } })
      .then((res) => res.json() as Promise<{ removed?: number; error?: string }>)
      .then((b) => setCleared(b.error ?? `Removed ${b.removed ?? 0} block(s).`))
      .catch((e: unknown) => setCleared(String(e)))
  }
  const cur = TOOLS.find((t) => t.tool === s.tool) ?? TOOLS[0]!
  const count = (w: WorldData) => `${w.objects.length.toLocaleString()} objects, ${w.objects.filter((o) => o.solid).length.toLocaleString()} solid, ${w.rivers.length} rivers, ${w.lakes.length} lakes`
  return (
    <>
      <section aria-label="Live world" className="cell py-2">
        <h2 className="lbl pb-1">Live world</h2>
        <p className="text-fg-2">
          {land?.recipe
            ? `Seed ${land.recipe.seed}, ${land.size} units across: ${count(land)}.`
            : land
              ? `A world file without a recipe (${count(land)}). Changing anything here makes a new one.`
              : 'No world file: the server is using the game file’s walls on flat ground. Change anything below to make one.'}
        </p>
      </section>

      <section aria-label="Recipe" className="cell py-2 grid grid-cols-2 gap-x-4 gap-y-2">
        <div className="flex items-end gap-1 min-w-0">
          <NumField label="Seed" value={r.seed} step={1} onCommit={(n) => s.set({ ...r, seed: Math.max(0, Math.floor(n)) })} className="flex-1" />
          <button type="button" className="btn" aria-label="New seed" title="New seed" onClick={() => s.set({ ...r, seed: Math.floor(Math.random() * 1e9) })}>
            <Shuffle {...ICON} aria-hidden />
          </button>
        </div>
        <SelectField
          label={r.size !== liveSize ? 'Size (restart to use)' : 'Size'}
          value={String(r.size)}
          options={SIZES.map((n) => ({ value: String(n), label: `${n} units` }))}
          onChange={(v) => s.set({ ...r, size: Number(v) })}
        />
        <Toggle label="Walled city" checked={r.city} onChange={(v) => s.set({ ...r, city: v })} />
        <Toggle label="Usual rivers" checked={r.rivers === null} onChange={(v) => s.set({ ...r, rivers: v ? null : 2 })} />
        {DIALS.map(([k, label]) => (
          <Range key={k} label={label} value={r[k]} min={0} max={3} step={0.25} mark={1} unit="×" onChange={(v) => s.set({ ...r, [k]: v })} />
        ))}
        {r.rivers !== null && <Range label="Rivers" value={r.rivers} min={0} max={12} step={1} onChange={(v) => s.set({ ...r, rivers: v })} />}
      </section>

      <section aria-label="Shape by hand" className="cell py-2">
        <h2 className="lbl pb-1">Shape by hand</h2>
        <div role="group" aria-label="Tool" className="flex flex-wrap items-end gap-1">
          {TOOLS.map((t) => (
            <button key={t.tool} type="button" className={`btn ${s.tool === t.tool ? 'border-accent text-fg' : ''}`} aria-pressed={s.tool === t.tool} onClick={() => s.setTool(t.tool)}>
              {t.icon}
              {t.label}
            </button>
          ))}
          {s.tool === 'add' && (
            <SelectField label="What" value={s.addType} options={ADDABLE.map((t) => ({ value: t, label: t }))} onChange={s.setAddType} className="w-28" />
          )}
        </div>
        <p className="text-fg-3 pt-1" aria-live="polite">
          {cur.hint}
        </p>
        {r.edits.length > 0 && (
          <ol className="pt-2" aria-label="Edits">
            {r.edits.map((e, i) => (
              <li key={i} className="flex items-center justify-between gap-2 border-t border-line py-1">
                <span className="text-fg-2 truncate">{describe(e)}</span>
                <button type="button" className="btn" aria-label={`Undo: ${describe(e)}`} onClick={() => s.set({ ...r, edits: r.edits.filter((_, k) => k !== i) })}>
                  <X {...ICON} aria-hidden />
                </button>
              </li>
            ))}
          </ol>
        )}
        <p className="text-fg-3 pt-1">Edits stay with the recipe: change the seed and they apply to the new land. A removal takes whatever is at that point.</p>
      </section>

      <section aria-label="Built blocks" className="cell py-2">
        <h2 className="lbl pb-1">Built blocks</h2>
        <p className="text-fg-2 pb-2" aria-live="polite">
          {cleared ?? `${blocks.toLocaleString()} block(s) built by players. They stay when the world changes, except where a new wall stands.`}
        </p>
        <div className="flex gap-2">
          {confirmClear ? (
            <>
              <button type="button" className="btn btn-primary" onClick={clearBlocks}>
                Remove all {blocks.toLocaleString()}
              </button>
              <button type="button" className="btn" onClick={() => setConfirmClear(false)}>
                Keep them
              </button>
            </>
          ) : (
            <button type="button" className="btn" disabled={blocks === 0} onClick={() => setConfirmClear(true)}>
              Clear all blocks
            </button>
          )}
        </div>
      </section>

      <section aria-label="Preview and apply" className="cell py-2">
        <p className="text-fg-2 pb-2" aria-live="polite">
          {s.error ? (
            <span className="text-accent">{s.error}</span>
          ) : s.busy ? (
            'Generating…'
          ) : s.preview ? (
            `Preview: ${count(s.preview)}.`
          ) : (
            (s.note ?? 'Showing the live world.')
          )}
        </p>
        <div className="flex gap-2">
          <button type="button" className="btn btn-primary" disabled={!s.dirty || !s.preview || s.busy || !!s.error} onClick={s.apply}>
            Apply to the live world
          </button>
          <button type="button" className="btn" disabled={!s.dirty} onClick={s.discard}>
            Discard
          </button>
        </div>
        <p className="text-fg-3 pt-2">Applying is live: nobody is disconnected, and anyone a new wall lands on is moved. The world is saved as world.json and used again after a restart.</p>
      </section>
    </>
  )
}
