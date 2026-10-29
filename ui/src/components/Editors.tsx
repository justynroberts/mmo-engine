import { Footprints, Pause, PersonSimpleRun, Target } from '@phosphor-icons/react'
import { Copy, Plus, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { CLASS_LOOKS, FX, GEAR_SLOTS, MATERIALS, RULE_DEFAULTS, type ActionFile, type Archetype, type AttrFile, type Behaviour, type ClassFile, type Emote, type GameFile, type LevelReward, type PlotRules, type GearSlot, type HoldKind, type ItemDef, type QuestFile, type QuestStep, type Rules } from '../api'
import { MAX_ITEMS, newItem, removeArch, removeItem, renameArch, renameItem } from '../directorEdit'
import {
  addFromTemplate,
  BOT_TEMPLATES,
  cleanAction,
  classProblems,
  cleanClass,
  cleanQuest,
  MAX_CLASSES,
  newClass,
  removeClass,
  renameClass,
  defaultQuestStep,
  MAX_QUEST_REWARDS,
  MAX_QUEST_STEPS,
  MAX_QUESTS,
  newQuest,
  QUEST_STEP_KINDS,
  questProblems,
  removeQuest,
  renameQuest,
  exportArchetype,
  importArchetypes,
  newAction,
  newArchetype,
  newAttr,
  removeAction,
  removeAttr,
  renameAction,
  renameAttr,
} from '../gameEdit'
import { Empty, NumField, SelectField, Strip, TextField, Toggle } from './Fields'

type Edit = { game: GameFile; setGame: (g: GameFile) => void }

const ICON = { size: 16, weight: 'regular' as const }
export const BEHAVIOUR_ICON: Record<Behaviour, JSX.Element> = {
  idle: <Pause {...ICON} aria-hidden />,
  wander: <Footprints {...ICON} aria-hidden />,
  chase: <Target {...ICON} aria-hidden />,
  flee: <PersonSimpleRun {...ICON} aria-hidden />,
}

function IconButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button type="button" className="btn px-1" aria-label={label} title={label} onClick={onClick}>
      {children}
    </button>
  )
}

function AddButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button type="button" className="btn mt-2" onClick={onClick}>
      <Plus size={16} strokeWidth={1.5} aria-hidden />
      {label}
    </button>
  )
}

// ------------------------------------------------------------------ settings

export function SettingsEditor({ game, setGame }: Edit) {
  const set = (k: 'move_speed' | 'road_speed' | 'entity_radius' | 'interest_radius' | 'max_visible') => (n: number) =>
    setGame({ ...game, [k]: n })
  return (
    <section aria-label="World settings" className="cell py-2">
      <h2 className="lbl pb-1">World settings</h2>
      <div className="grid grid-cols-6 gap-x-2">
        <NumField label="Speed u/s" value={game.move_speed} onCommit={set('move_speed')} />
        <NumField label="On roads ×" value={game.road_speed ?? 1.5} onCommit={(n) => set('road_speed')(Math.min(4, Math.max(1, n)))} />
        <NumField label="Radius" value={game.entity_radius} onCommit={set('entity_radius')} />
        <NumField label="Interest" value={game.interest_radius} onCommit={set('interest_radius')} />
        <NumField label="Max seen" value={game.max_visible} onCommit={set('max_visible')} />
        <Toggle label="Players fight (PvP)" checked={game.pvp !== false} onChange={(on) => setGame({ ...game, pvp: on ? undefined : false })} />
      </div>
      <div className="grid grid-cols-6 gap-x-2 pt-2">
        <SelectField
          label="Currency"
          value={game.currency ?? ''}
          options={[{ value: '', label: 'none' }, ...(game.items ?? []).map((x) => ({ value: x.name, label: x.name }))]}
          onChange={(v) => {
            const next: GameFile = { ...game, currency: v }
            if (v === '') delete next.currency
            setGame(next)
          }}
          className="col-span-2"
        />
      </div>
      <div className="grid grid-cols-6 gap-x-2 pt-2">
        <Toggle
          label="Levels"
          checked={!!game.levels}
          onChange={(on) => setGame({ ...game, levels: on ? { cap: 20, base: 100, exponent: 1.6, might: 0.06, toughness: 0.05, discover_xp: 10 } : undefined })}
        />
        {game.levels && (
          <>
            <NumField label="Level cap" value={game.levels.cap ?? 20} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, cap: Math.min(100, Math.max(1, Math.round(n))) } })} />
            <NumField label="XP for level 2" value={game.levels.base ?? 100} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, base: Math.max(1, n) } })} />
            <NumField label="Curve" value={game.levels.exponent ?? 1.6} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, exponent: Math.min(4, Math.max(0.5, n)) } })} />
            <NumField label="Might / level" value={game.levels.might ?? 0.06} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, might: Math.min(1, Math.max(0, n)) } })} />
            <NumField label="Toughness / level" value={game.levels.toughness ?? 0.05} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, toughness: Math.min(1, Math.max(0, n)) } })} />
            <NumField label="XP for a new place" value={game.levels.discover_xp ?? 10} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, discover_xp: Math.max(0, Math.round(n)) } })} />
            <NumField label="Life XP / 100 walked" value={game.levels.life?.walk ?? 1.5} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, walk: Math.min(1000, Math.max(0, n)) } } })} />
            <NumField label="Life XP / action" value={game.levels.life?.act ?? 0.5} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, act: Math.min(1000, Math.max(0, n)) } } })} />
            <NumField label="Life XP / deed" value={game.levels.life?.deed ?? 1} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, deed: Math.min(1000, Math.max(0, n)) } } })} />
            <NumField label="Life XP / planting" value={game.levels.life?.plant ?? 1} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, plant: Math.min(1000, Math.max(0, n)) } } })} />
            <NumField label="Life XP / harvest" value={game.levels.life?.harvest ?? 3} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, harvest: Math.min(1000, Math.max(0, n)) } } })} />
            <NumField label="Life XP / minute" value={game.levels.life?.minute ?? 1} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, minute: Math.min(1000, Math.max(0, n)) } } })} />
            <NumField label="+ / minute in company" value={game.levels.life?.company ?? 1} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, company: Math.min(1000, Math.max(0, n)) } } })} />
            <NumField label="Life XP cap / min" value={game.levels.life?.per_min ?? 30} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, per_min: Math.max(0, Math.round(n)) } } })} />
            <NumField label="Trail (cells that stop paying)" value={game.levels.life?.trail ?? 64} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, trail: Math.min(1024, Math.max(0, Math.round(n))) } } })} />
            <NumField label="Repeats that stop paying" value={game.levels.life?.repeat ?? 8} onCommit={(n) => setGame({ ...game, levels: { ...game.levels, life: { ...game.levels?.life, repeat: Math.min(64, Math.max(0, Math.round(n))) } } })} />
          </>
        )}
      </div>
      {game.levels && <LevelRewards game={game} setGame={setGame} />}
      <RulesFields game={game} setGame={setGame} />
      <PlotsFields game={game} setGame={setGame} />
      <EmotesFields game={game} setGame={setGame} />
    </section>
  )
}

/** The tuning numbers, by section. A value back at its default is dropped, so the file keeps only what was changed. */
const RULE_FIELDS: [string, [keyof Rules, string][]][] = [
  ['Reach', [['talk_reach', 'Talk'], ['pickup_reach', 'Pick up'], ['say_reach', 'Say'], ['shout_reach', 'Shout (0 all)'], ['remark_reach', 'Bot remark'], ['market_reach', 'Market']]],
  ['Pace', [['global_cooldown_secs', 'Global cd s'], ['item_cooldown_secs', 'Item cd s'], ['ground_item_secs', 'Ground items s'], ['bot_say_secs', 'Bot talks every s'], ['dungeon_idle_secs', 'Empty dungeon s']]],
  ['Fights', [['sneak_sight', 'Sneak sight ×'], ['chase_give_up_secs', 'Chase gives up s'], ['pvp_quiet_secs', 'PvP calm s'], ['kill_credit_secs', 'Kill credit s']]],
  ['Falling', [['fall_xp_loss_pct', 'XP lost %'], ['fall_coin_loss_pct', 'Coin lost %'], ['fall_drop_pct', 'Bag dropped %'], ['rise_delay_secs', 'Rise after s']]],
  ['Hailing', [['hail_reach', 'Hail reach'], ['hail_secs', 'Stands for s']]],
  ['Wandering', [['wander_linger_secs', 'Lingers s'], ['wander_range', 'Range (no home)'], ['remark_pause_secs', 'Stops to talk s']]],
  ['The dark', [['night_sight', 'Fighters see ×'], ['night_ambush_pct', 'Ambush % a minute'], ['night_pack', 'Most in a pack']]],
  ['Town life', [['brawl_pct', 'Brawl % a minute'], ['lift_pct', 'Cutpurse % a second'], ['inn_pct', 'To the inn % an evening'], ['newcomer_secs', 'Newcomer after (s, 0 = none)'], ['plea_pct', 'Asks for help % a minute']]],
  ['Seasons', [['season_days', 'Days a season (0 = none)'], ['season_shift', 'Shift the year (days)']]],
  ['Company', [['party_size', 'Party size'], ['party_share_reach', 'Party XP reach'], ['quest_share_reach', 'Quest share reach'], ['guild_size', 'Guild size']]],
  ['Trade', [['trade_reach', 'Trade reach'], ['trade_break', 'Trade breaks at'], ['listing_hours', 'Listing hours'], ['listings_per_seller', 'Listings each']]],
]

function LevelRewards({ game, setGame }: Edit) {
  const levels = game.levels ?? {}
  const rewards = levels.rewards ?? []
  const items = game.items ?? []
  const cap = levels.cap ?? 20
  const put = (next: LevelReward[]) => {
    const l: NonNullable<GameFile['levels']> = { ...levels, rewards: [...next].sort((a, b) => a.level - b.level) }
    if (next.length === 0) delete l.rewards
    setGame({ ...game, levels: l })
  }
  const patch = (i: number, p: Partial<LevelReward>) => put(rewards.map((r, j) => (j === i ? { ...r, ...p } : r)))
  return (
    <div className="pt-2" aria-label="Level rewards">
      <h2 className="lbl pb-1">Level rewards: what reaching a level gives</h2>
      {rewards.map((r, i) => {
        const st = r.items?.[0]
        return (
          <div key={i} className="grid grid-cols-12 gap-x-2 items-end">
            <NumField label="Level" value={r.level} onCommit={(n) => patch(i, { level: Math.min(cap, Math.max(2, Math.round(n))) })} className="col-span-1" />
            <SelectField
              label="Item"
              value={st?.item ?? ''}
              options={[{ value: '', label: 'nothing' }, ...items.map((x) => ({ value: x.name, label: x.name }))]}
              onChange={(v) => patch(i, { items: v ? [{ item: v, count: st?.count ?? 1 }, ...(r.items ?? []).slice(1)] : (r.items ?? []).slice(1) })}
              className="col-span-3"
            />
            <NumField label="Count" value={st?.count ?? 1} onCommit={(n) => st && patch(i, { items: [{ ...st, count: Math.min(255, Math.max(1, Math.round(n))) }, ...(r.items ?? []).slice(1)] })} className="col-span-1" />
            <TextField label="Says" value={r.text ?? ''} maxLength={200} allowEmpty onCommit={(v) => patch(i, { text: v || undefined })} className="col-span-6" />
            <IconButton label={`Remove the level ${r.level} reward`} onClick={() => put(rewards.filter((_, j) => j !== i))}>
              <Trash2 size={16} strokeWidth={1.5} aria-hidden />
            </IconButton>
          </div>
        )
      })}
      {rewards.length < cap - 1 && (
        <AddButton label="Add a level reward" onClick={() => put([...rewards, { level: Math.min(cap, (rewards.at(-1)?.level ?? 1) + 1), text: '' }])} />
      )}
    </div>
  )
}

function PlotsFields({ game, setGame }: Edit) {
  const p = game.plots
  const put = (next: PlotRules | undefined) => {
    const g: GameFile = { ...game, plots: next }
    if (!next) delete g.plots
    setGame(g)
  }
  const whole = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, Math.round(n)))
  const sizes = p?.sizes ?? []
  const setSizes = (next: { size: number; cost?: number }[]) => {
    const q: PlotRules = { ...p, sizes: next }
    if (next.length === 0) delete q.sizes
    put(q)
  }
  return (
    <div className="pt-2" aria-label="Land">
      <div className="grid grid-cols-6 gap-x-2 items-end">
        <Toggle label="Land: /claim" checked={!!p} onChange={(on) => put(on ? { size: 16, per_player: 1 } : undefined)} />
        {p && (
          <>
            <NumField label="Land: size" value={p.size ?? 16} onCommit={(n) => put({ ...p, size: whole(n, 4, 128) })} />
            <NumField label="Land: each" value={p.per_player ?? 1} onCommit={(n) => put({ ...p, per_player: whole(n, 1, 8) })} />
            <NumField label="Land: cost" value={p.cost ?? 0} onCommit={(n) => put({ ...p, cost: whole(n, 0, 1_000_000) || undefined })} />
          </>
        )}
      </div>
      {p &&
        sizes.map((s, i) => (
          <div key={i} className="grid grid-cols-6 gap-x-2 items-end">
            <NumField label={`Grow ${i + 1}: size`} value={s.size} onCommit={(n) => setSizes(sizes.map((x, j) => (j === i ? { ...x, size: whole(n, 5, 128) } : x)))} />
            <NumField label={`Grow ${i + 1}: cost`} value={s.cost ?? 0} onCommit={(n) => setSizes(sizes.map((x, j) => (j === i ? { ...x, cost: whole(n, 0, 1_000_000) || undefined } : x)))} />
            <NumField label={`Grow ${i + 1}: level`} value={s.level ?? 0} onCommit={(n) => setSizes(sizes.map((x, j) => (j === i ? { ...x, level: whole(n, 0, 100) || undefined } : x)))} />
            <IconButton label={`Remove grow size ${i + 1}`} onClick={() => setSizes(sizes.filter((_, j) => j !== i))}>
              <Trash2 size={16} strokeWidth={1.5} aria-hidden />
            </IconButton>
          </div>
        ))}
      {p && sizes.length < 8 && <AddButton label="Add a bigger size" onClick={() => setSizes([...sizes, { size: Math.min(128, (sizes.at(-1)?.size ?? p.size ?? 16) * 2) }])} />}
    </div>
  )
}

function EmotesFields({ game, setGame }: Edit) {
  const emotes = game.emotes ?? []
  const put = (next: Emote[]) => {
    const g: GameFile = { ...game, emotes: next }
    if (next.length === 0) delete g.emotes
    setGame(g)
  }
  const patch = (i: number, p: Partial<Emote>) => {
    const e = { ...emotes[i]!, ...p }
    if (!e.at) delete e.at
    if (!e.anim) delete e.anim
    if (!e.stays) delete e.stays
    put(emotes.map((x, j) => (j === i ? e : x)))
  }
  const fresh = () => {
    let n = 1
    while (emotes.some((e) => e.cmd === `emote${n}`)) n++
    return `emote${n}`
  }
  return (
    <div className="pt-2" aria-label="Emotes">
      <h2 className="lbl pb-1">Emotes: /cmd for those nearby; {'{name}'} acts it, {'{target}'} is who it is at</h2>
      {emotes.map((e, i) => (
        <div key={i} className="grid grid-cols-12 gap-x-2 items-end">
          <TextField
            label="Command"
            value={e.cmd}
            maxLength={16}
            invalid={emotes.some((x, j) => j !== i && x.cmd === e.cmd)}
            onCommit={(v) => patch(i, { cmd: v.toLowerCase().replace(/[^a-z0-9]/g, '') || e.cmd })}
            className="col-span-2"
          />
          <TextField label="Says" value={e.text} maxLength={100} onCommit={(v) => patch(i, { text: v })} className="col-span-3" />
          <TextField label="At someone" value={e.at ?? ''} maxLength={100} allowEmpty onCommit={(v) => patch(i, { at: v })} className="col-span-3" />
          <Toggle label="Held" checked={!!e.stays} onChange={(on) => patch(i, { stays: on })} />
          <TextField label="Clip" value={e.anim ?? ''} maxLength={32} allowEmpty onCommit={(v) => patch(i, { anim: v.replace(/\|/g, '') })} className="col-span-2" />
          <IconButton label={`Remove /${e.cmd}`} onClick={() => put(emotes.filter((_, j) => j !== i))}>
            <Trash2 size={16} strokeWidth={1.5} aria-hidden />
          </IconButton>
        </div>
      ))}
      <AddButton label="Add emote" onClick={() => put([...emotes, { cmd: fresh(), text: '{name} waves.' }])} />
    </div>
  )
}

function RulesFields({ game, setGame }: Edit) {
  const set = (k: keyof Rules) => (n: number) => {
    const rules: Partial<Rules> = { ...game.rules, [k]: n }
    if (!Number.isFinite(n) || n === RULE_DEFAULTS[k]) delete rules[k]
    const next: GameFile = { ...game, rules }
    if (Object.keys(rules).length === 0) delete next.rules
    setGame(next)
  }
  return (
    <>
      {RULE_FIELDS.map(([title, fields]) => (
        <div key={title} className="grid grid-cols-6 gap-x-2 pt-2" aria-label={`${title} rules`}>
          {fields.map(([k, label]) => (
            <NumField key={k} label={`${title}: ${label}`} value={game.rules?.[k] ?? RULE_DEFAULTS[k]} onCommit={set(k)} />
          ))}
        </div>
      ))}
    </>
  )
}

// ---------------------------------------------------------------- attributes

export function AttrsEditor({ game, setGame }: Edit) {
  const patch = (i: number, p: Partial<AttrFile>) =>
    setGame({ ...game, attrs: game.attrs.map((a, k) => (k === i ? { ...a, ...p } : a)) })
  const dup = (name: string, i: number) => game.attrs.some((a, k) => k !== i && a.name === name)
  return (
    <div>
      <SettingsEditor game={game} setGame={setGame} />
      {game.attrs.length === 0 && <Empty>No attributes. Health is just one of these; add whatever your game needs.</Empty>}
      {game.attrs.map((a, i) => (
        <Strip key={i}>
          <div className="flex items-end gap-2">
            <TextField
              label={`Attribute ${i}`}
              value={a.name}
              invalid={dup(a.name, i)}
              onCommit={(n) => setGame(renameAttr(game, i, n))}
              className="flex-1"
            />
            <IconButton label={`Delete attribute ${a.name}`} onClick={() => setGame(removeAttr(game, i))}>
              <Trash2 size={16} strokeWidth={1.5} aria-hidden />
            </IconButton>
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-2">
            <NumField label="Min" value={a.min} onCommit={(n) => patch(i, { min: n })} />
            <NumField label="Max" value={a.max} onCommit={(n) => patch(i, { max: n })} />
            <NumField label="Start" value={a.default} onCommit={(n) => patch(i, { default: n })} />
            <NumField label="Regen /s" value={a.regen_per_sec} onCommit={(n) => patch(i, { regen_per_sec: n })} />
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-1">
            <SelectField
              label="Bits"
              value={String(a.bits) as '8' | '16'}
              options={[
                { value: '8', label: '8' },
                { value: '16', label: '16' },
              ]}
              onChange={(v) => patch(i, { bits: v === '16' ? 16 : 8 })}
            />
            <Toggle label="Sent" checked={a.replicated} onChange={(v) => patch(i, { replicated: v })} />
            <SelectField
              label="At min"
              value={a.on_zero}
              options={[
                { value: 'none', label: 'nothing' },
                { value: 'respawn', label: 'respawn' },
              ]}
              onChange={(v) => patch(i, { on_zero: v })}
              className="col-span-2"
            />
          </div>
        </Strip>
      ))}
      <AddButton label="Add attribute" onClick={() => setGame({ ...game, attrs: [...game.attrs, newAttr(game)] })} />
    </div>
  )
}

// ------------------------------------------------------------------- actions

export function ActionsEditor({ game, setGame }: Edit) {
  const attrNames = game.attrs.map((a) => ({ value: a.name, label: a.name }))
  // Every edit goes through cleanAction, so a spell field set back to its default is dropped, not written.
  const patch = (i: number, p: Partial<ActionFile>) =>
    setGame({ ...game, actions: game.actions.map((a, k) => (k === i ? cleanAction({ ...a, ...p }) : a)) })
  const first = game.attrs[0]?.name ?? ''
  return (
    <div>
      <p className="text-fg-3 pb-1">
        Spell fields (aim, area, hits, hold, look, bolt, school, effect seconds) apply live. A replicated attribute called{' '}
        <span className="text-fg">held</span> (0..3) shows who is held: 1 mez, 2 stun, 3 root.
      </p>
      {game.actions.length === 0 && <Empty>No actions. Players and bots can only move.</Empty>}
      {game.actions.map((a, i) => (
        <Strip key={i}>
          <div className="flex items-end gap-2">
            <TextField label="Action" value={a.name} onCommit={(n) => setGame(renameAction(game, i, n))} className="flex-1" />
            <IconButton label={`Delete action ${a.name}`} onClick={() => setGame(removeAction(game, i))}>
              <Trash2 size={16} strokeWidth={1.5} aria-hidden />
            </IconButton>
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-2">
            <NumField label="Id" value={a.id} onCommit={(n) => patch(i, { id: Math.round(n) })} />
            <NumField label="Cooldown ticks" value={a.cooldown_ticks} onCommit={(n) => patch(i, { cooldown_ticks: Math.max(0, Math.round(n)) })} />
            <NumField label="Range" value={a.range} onCommit={(n) => patch(i, { range: n })} />
            <TextField
              label="School"
              value={a.school ?? ''}
              allowEmpty
              maxLength={24}
              onCommit={(t) => patch(i, { school: t.trim().slice(0, 24) || undefined })}
            />
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-1">
            <SelectField
              label="Aim"
              value={a.aim ?? 'target'}
              options={[
                { value: 'target', label: 'target' },
                { value: 'self', label: 'self' },
              ]}
              onChange={(v) => patch(i, { aim: v })}
            />
            <SelectField
              label="Hits"
              value={a.hits ?? 'any'}
              options={[
                { value: 'any', label: 'anyone' },
                { value: 'foes', label: 'foes' },
                { value: 'friends', label: 'friends' },
              ]}
              onChange={(v) => patch(i, { hits: v })}
            />
            <NumField label="Area radius" value={a.area ?? 0} onCommit={(n) => patch(i, { area: Math.min(64, Math.max(0, n)) })} />
            <SelectField
              label="Look"
              value={a.fx ?? ''}
              options={[{ value: '', label: 'plain' }, ...FX.map((f) => ({ value: f, label: f }))]}
              onChange={(v) => patch(i, { fx: v || undefined })}
            />
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-1">
            <SelectField
              label="Hold"
              value={a.hold?.kind ?? ''}
              options={[
                { value: '', label: 'none' },
                { value: 'mez', label: 'mez' },
                { value: 'stun', label: 'stun' },
                { value: 'root', label: 'root' },
              ]}
              onChange={(v: HoldKind | '') => patch(i, { hold: v ? { kind: v, secs: a.hold?.secs ?? 3 } : undefined })}
            />
            {a.hold ? (
              <NumField
                label="Hold secs"
                value={a.hold.secs}
                onCommit={(n) => a.hold && patch(i, { hold: { kind: a.hold.kind, secs: Math.min(30, Math.max(0.1, n)) } })}
              />
            ) : (
              <div />
            )}
            <Toggle label="Bolt" checked={!!a.bolt} onChange={(v) => patch(i, { bolt: v })} />
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-1">
            <div className="col-span-3">
              <TextField
                label="Description (players' tooltip)"
                value={a.desc ?? ''}
                allowEmpty
                maxLength={200}
                onCommit={(t) => patch(i, { desc: t.replace(/\s+/g, ' ').trim().slice(0, 200) || undefined })}
              />
            </div>
            <NumField label="Levitate secs" value={a.lift ?? 0} onCommit={(n) => patch(i, { lift: Math.min(300, Math.max(0, n)) || undefined })} />
          </div>
          <div className="grid grid-cols-4 gap-x-2 pt-1">
            <NumField label="Learnt at level" value={a.level ?? 1} onCommit={(n) => patch(i, { level: Math.min(100, Math.max(1, Math.round(n))) })} />
          </div>
          <div className="pt-1">
            <p className="lbl">Needs</p>
            {a.requires ? (
              <div className="flex items-end gap-2">
                <SelectField
                  label="Own attribute"
                  value={a.requires.attr}
                  options={attrNames}
                  onChange={(v) => patch(i, { requires: { attr: v, min: a.requires?.min ?? 0 } })}
                  className="flex-1"
                />
                <NumField
                  label="At least"
                  value={a.requires.min}
                  onCommit={(n) => patch(i, { requires: { attr: a.requires?.attr ?? first, min: n } })}
                  className="w-24"
                />
                <IconButton label="Remove requirement" onClick={() => patch(i, { requires: null })}>
                  <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                </IconButton>
              </div>
            ) : (
              <button type="button" className="btn" disabled={!first} onClick={() => patch(i, { requires: { attr: first, min: 1 } })}>
                <Plus size={16} strokeWidth={1.5} aria-hidden />
                Add requirement
              </button>
            )}
          </div>
          <div className="pt-1">
            <p className="lbl">Effects</p>
            {a.effects.map((e, k) => (
              <div key={k} className="flex items-end gap-2">
                <SelectField
                  label="On"
                  value={e.to}
                  options={[
                    { value: 'target', label: 'target' },
                    { value: 'self', label: 'self' },
                  ]}
                  onChange={(v) => patch(i, { effects: a.effects.map((x, j) => (j === k ? { ...x, to: v } : x)) })}
                  className="w-24"
                />
                <SelectField
                  label="Attribute"
                  value={e.attr}
                  options={attrNames}
                  onChange={(v) => patch(i, { effects: a.effects.map((x, j) => (j === k ? { ...x, attr: v } : x)) })}
                  className="flex-1"
                />
                <NumField
                  label="Add"
                  value={e.delta}
                  onCommit={(n) => patch(i, { effects: a.effects.map((x, j) => (j === k ? { ...x, delta: n } : x)) })}
                  className="w-24"
                />
                <NumField
                  label="Over secs"
                  value={e.secs ?? 0}
                  onCommit={(n) =>
                    patch(i, { effects: a.effects.map((x, j) => (j === k ? { ...x, secs: Math.min(60, Math.max(0, n)) } : x)) })
                  }
                  className="w-20"
                />
                <NumField
                  label="Lasts secs"
                  value={e.lasts ?? 0}
                  onCommit={(n) =>
                    patch(i, { effects: a.effects.map((x, j) => (j === k ? { ...x, lasts: Math.min(300, Math.max(0, n)) } : x)) })
                  }
                  className="w-20"
                />
                <IconButton label="Remove effect" onClick={() => patch(i, { effects: a.effects.filter((_, j) => j !== k) })}>
                  <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                </IconButton>
              </div>
            ))}
            <button
              type="button"
              className="btn mt-1"
              disabled={!first}
              onClick={() => patch(i, { effects: [...a.effects, { to: 'target', attr: first, delta: -1 }] })}
            >
              <Plus size={16} strokeWidth={1.5} aria-hidden />
              Add effect
            </button>
          </div>
        </Strip>
      ))}
      <AddButton label="Add action" onClick={() => setGame({ ...game, actions: [...game.actions, newAction(game)] })} />
    </div>
  )
}

// --------------------------------------------------------------------- items

/** What a new character starts with: a stack per row. */
function StarterKit({ game, setGame }: Edit) {
  const items = game.items ?? []
  const kit = game.starter ?? []
  const put = (next: { item: string; count?: number }[]) => {
    const g: GameFile = { ...game, starter: next }
    if (next.length === 0) delete g.starter
    setGame(g)
  }
  return (
    <Strip>
      <h2 className="lbl pb-1">Starter kit: what a new character carries</h2>
      {kit.map((s, i) => (
        <div key={i} className="grid grid-cols-6 gap-x-2 items-end">
          <SelectField
            label="Item"
            value={s.item}
            options={items.map((x) => ({ value: x.name, label: x.name }))}
            onChange={(v) => put(kit.map((k, j) => (j === i ? { ...k, item: v } : k)))}
            className="col-span-3"
          />
          <NumField label="Count" value={s.count ?? 1} onCommit={(n) => put(kit.map((k, j) => (j === i ? { ...k, count: Math.max(1, Math.min(99, Math.round(n))) } : k)))} />
          <IconButton label={`Remove ${s.item} from the starter kit`} onClick={() => put(kit.filter((_, j) => j !== i))}>
            <Trash2 size={16} strokeWidth={1.5} aria-hidden />
          </IconButton>
        </div>
      ))}
      {items.length > 0 && kit.length < 24 && <AddButton label="Add to starter kit" onClick={() => put([...kit, { item: items[0]?.name ?? "", count: 1 }])} />}
    </Strip>
  )
}

export function ItemsEditor({ game, setGame }: Edit) {
  const items = game.items ?? []
  const attrNames = game.attrs.map((a) => ({ value: a.name, label: a.name }))
  const first = game.attrs[0]?.name ?? ''
  const patch = (i: number, p: Partial<ItemDef>) => setGame({ ...game, items: items.map((a, k) => (k === i ? { ...a, ...p } : a)) })
  const dup = (name: string, i: number) => items.some((a, k) => k !== i && a.name === name)
  return (
    <div>
      <StarterKit game={game} setGame={setGame} />
      {items.length === 0 && <Empty>No items. Items are carried in an 8 slot bag, used for their effects, dropped, and left behind by bots.</Empty>}
      {items.map((it, i) => (
        <Strip key={i}>
          <div className="flex items-end gap-2">
            <label className="block">
              <span className="lbl block">Colour</span>
              <input
                type="color"
                value={it.color}
                aria-label={`Colour of ${it.name}`}
                onChange={(e) => patch(i, { color: e.target.value })}
                className="block h-6 w-8 cursor-pointer border border-line bg-transparent p-0 rounded-sm"
              />
            </label>
            <TextField label="Item" value={it.name} invalid={dup(it.name, i)} onCommit={(n) => setGame(renameItem(game, i, n))} className="flex-1" />
            <TextField label="Shown as" value={it.title ?? ''} onCommit={(t) => patch(i, { title: t.trim().slice(0, 32) || undefined })} className="w-32" />
            <IconButton label={`Delete item ${it.name}`} onClick={() => setGame(removeItem(game, i))}>
              <Trash2 size={16} strokeWidth={1.5} aria-hidden />
            </IconButton>
          </div>
          <div className="grid grid-cols-3 gap-x-2 pt-2 items-end">
            <NumField label="Stack size" value={it.stack_max} onCommit={(n) => patch(i, { stack_max: Math.max(1, Math.min(99, Math.round(n))) })} />
            <div className="col-span-2 pb-1">
              <Toggle label="Used up when used" checked={it.consume} onChange={(v) => patch(i, { consume: v })} />
            </div>
          </div>
          <div className="grid grid-cols-3 gap-x-2 pt-1 items-end">
            <NumField label="Merchant pays" value={it.value ?? 0} onCommit={(n) => patch(i, { value: Math.max(0, Math.min(65535, Math.round(n))) || undefined })} />
            <NumField label="Mount: speed +" value={it.mount ?? 0} onCommit={(n) => patch(i, { mount: Math.max(0, Math.min(4, n)) || undefined })} />
            <SelectField
              label="Block of"
              value={it.block ?? ''}
              options={[{ value: '', label: 'none' }, ...MATERIALS.filter((m) => m === it.block || !(game.items ?? []).some((o, j) => j !== i && o.block === m)).map((m) => ({ value: m, label: m }))]}
              onChange={(v) => patch(i, { block: v === '' ? undefined : v })}
            />
          </div>
          <div className="grid grid-cols-3 gap-x-2 pt-1 items-end">
            <SelectField
              label="Gear"
              value={it.equip?.slot ?? ''}
              options={[{ value: '', label: 'none' }, ...GEAR_SLOTS.map((x) => ({ value: x, label: x }))]}
              onChange={(v) => patch(i, { equip: v === '' ? undefined : { ...it.equip, slot: v as GearSlot } })}
            />
            {it.equip && (
              <>
                <NumField
                  label="Might +"
                  value={it.equip.might ?? 0}
                  onCommit={(n) => patch(i, { equip: { ...it.equip!, might: Math.max(0, Math.min(2, n)) || undefined } })}
                />
                <NumField
                  label="Toughness +"
                  value={it.equip.toughness ?? 0}
                  onCommit={(n) => patch(i, { equip: { ...it.equip!, toughness: Math.max(0, Math.min(2, n)) || undefined } })}
                />
              </>
            )}
          </div>
          <div className="pt-1">
            <p className="lbl">When used</p>
            {it.uses.length === 0 && <p className="m-0 text-fg-3">Does nothing. Fine for trade goods and keepsakes.</p>}
            {it.uses.map((u, k) => (
              <div key={k} className="flex items-end gap-2">
                <SelectField
                  label="Attribute"
                  value={u.attr}
                  options={attrNames}
                  onChange={(v) => patch(i, { uses: it.uses.map((x, j) => (j === k ? { ...x, attr: v } : x)) })}
                  className="flex-1"
                />
                <NumField label="Add" value={u.delta} onCommit={(n) => patch(i, { uses: it.uses.map((x, j) => (j === k ? { ...x, delta: n } : x)) })} className="w-24" />
                <IconButton label="Remove effect" onClick={() => patch(i, { uses: it.uses.filter((_, j) => j !== k) })}>
                  <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                </IconButton>
              </div>
            ))}
            <button type="button" className="btn mt-1" disabled={!first || it.uses.length >= 4} onClick={() => patch(i, { uses: [...it.uses, { attr: first, delta: 10 }] })}>
              <Plus size={16} strokeWidth={1.5} aria-hidden />
              Add effect
            </button>
          </div>
        </Strip>
      ))}
      <AddButton label="Add item" onClick={() => items.length < MAX_ITEMS && setGame({ ...game, items: [...items, newItem(game)] })} />
      <p className="m-0 pt-3 text-label text-fg-3 max-w-[40rem]">
        Items are not part of the wire format, so adding or changing them never disconnects anyone. Give bots things to drop in the Bots tab; players pick
        them up with E and use them with Shift and a number.
      </p>
    </div>
  )
}

// -------------------------------------------------------------------- quests

export function QuestsEditor({ game, setGame }: Edit) {
  const quests = game.quests ?? []
  // Every edit goes through cleanQuest, so an optional field cleared or set back to its default is left out.
  const patch = (i: number, p: Partial<QuestFile>) => setGame({ ...game, quests: quests.map((q, k) => (k === i ? cleanQuest({ ...q, ...p }) : q)) })
  const dup = (name: string, i: number) => quests.some((q, k) => k !== i && q.name === name)
  const opts = (names: string[]) => names.map((n) => ({ value: n, label: n }))
  const bots = opts(game.archetypes.map((a) => a.name))
  const places = opts(game.places.map((p) => p.name))
  const items = opts((game.items ?? []).map((x) => x.name))
  // A select must show its current value even when it names something that has gone (the problem list says so).
  const withValue = (o: { value: string; label: string }[], v: string) => (v && !o.some((x) => x.value === v) ? [{ value: v, label: `${v} (missing)` }, ...o] : o)
  const problems = questProblems(game)
  const fresh = newQuest(game)
  return (
    <div>
      <p className="text-fg-3 pb-1">
        Quests apply live: nobody is disconnected. A bot type gives each one; players talk to it, do the steps in order, then hand it in for the experience and items.
      </p>
      {quests.length === 0 && <Empty>No quests. Add one, give it to a bot type, and players can take it by talking to those bots.</Empty>}
      {quests.map((q, i) => {
        const setStep = (k: number, st: QuestStep) => patch(i, { steps: q.steps.map((x, j) => (j === k ? st : x)) })
        const rewards = q.reward ?? []
        const setReward = (k: number, p: Partial<{ item: string; count: number }>) => patch(i, { reward: rewards.map((x, j) => (j === k ? { ...x, ...p } : x)) })
        return (
          <Strip key={i}>
            <div className="flex items-end gap-2">
              <TextField label="Quest" value={q.name} invalid={dup(q.name, i)} maxLength={48} onCommit={(n) => setGame(renameQuest(game, i, n.trim()))} className="w-48" />
              <TextField label="Title (players see this)" value={q.title} maxLength={64} onCommit={(t) => patch(i, { title: t.slice(0, 64) })} className="flex-1" />
              <IconButton label={`Delete quest ${q.name}`} onClick={() => setGame(removeQuest(game, i))}>
                <Trash2 size={16} strokeWidth={1.5} aria-hidden />
              </IconButton>
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-2">
              <SelectField label="Given by" value={q.giver} options={withValue(bots, q.giver)} onChange={(v) => patch(i, { giver: v })} />
              <SelectField
                label="Handed in to"
                value={q.turn_in ?? ''}
                options={[{ value: '', label: 'the giver' }, ...withValue(bots, q.turn_in ?? '')]}
                onChange={(v) => patch(i, { turn_in: v || undefined })}
              />
              <SelectField
                label="After quest"
                value={q.after ?? ''}
                options={[{ value: '', label: 'none' }, ...withValue(opts(quests.filter((o) => o.name !== q.name).map((o) => o.name)), q.after ?? '')]}
                onChange={(v) => patch(i, { after: v || undefined })}
              />
              <NumField label="From level" value={q.level ?? 1} onCommit={(n) => patch(i, { level: Math.min(100, Math.max(1, Math.round(n))) })} />
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-1">
              <div className="col-span-4">
                <TextField label="Offer (what the giver says)" value={q.text} allowEmpty maxLength={600} onCommit={(t) => patch(i, { text: t.slice(0, 600) })} />
              </div>
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-1">
              <div className="col-span-4">
                <TextField label="On hand-in (optional)" value={q.done_text ?? ''} allowEmpty maxLength={600} onCommit={(t) => patch(i, { done_text: t.slice(0, 600) || undefined })} />
              </div>
            </div>
            <div className="pt-1">
              <p className="lbl">Steps, in order</p>
              {q.steps.map((st, k) => (
                <div key={k} className="flex items-end gap-2">
                  <SelectField
                    label={`Step ${k + 1}`}
                    value={st.kind}
                    options={QUEST_STEP_KINDS.map((x) => ({ value: x.kind, label: x.label }))}
                    onChange={(v) => v !== st.kind && setStep(k, defaultQuestStep(v, game))}
                    className="w-36"
                  />
                  {(st.kind === 'kill' || st.kind === 'talk') && (
                    <SelectField label="Bot type" value={st.archetype} options={withValue(bots, st.archetype)} onChange={(v) => setStep(k, { ...st, archetype: v })} className="flex-1" />
                  )}
                  {st.kind === 'visit' && <SelectField label="Place" value={st.place} options={withValue(places, st.place)} onChange={(v) => setStep(k, { ...st, place: v })} className="flex-1" />}
                  {st.kind === 'collect' && <SelectField label="Item" value={st.item} options={withValue(items, st.item)} onChange={(v) => setStep(k, { ...st, item: v })} className="flex-1" />}
                  {st.kind === 'kill' && (
                    <NumField label="How many" value={st.count} onCommit={(n) => setStep(k, { ...st, count: Math.min(65535, Math.max(1, Math.round(n))) })} className="w-20" />
                  )}
                  {st.kind === 'collect' && (
                    <NumField label="How many" value={st.count} onCommit={(n) => setStep(k, { ...st, count: Math.min(255, Math.max(1, Math.round(n))) })} className="w-20" />
                  )}
                  {q.steps.length > 1 && (
                    <IconButton label={`Remove step ${k + 1}`} onClick={() => patch(i, { steps: q.steps.filter((_, j) => j !== k) })}>
                      <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                    </IconButton>
                  )}
                </div>
              ))}
              <button
                type="button"
                className="btn mt-1"
                disabled={q.steps.length >= MAX_QUEST_STEPS}
                onClick={() => patch(i, { steps: [...q.steps, defaultQuestStep(game.places[0] ? 'visit' : 'kill', game)] })}
              >
                <Plus size={16} strokeWidth={1.5} aria-hidden />
                Add step
              </button>
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-2 items-end">
              <NumField label="Experience" value={q.xp ?? 0} onCommit={(n) => patch(i, { xp: Math.max(0, Math.round(n)) || undefined })} />
              <div className="pb-1">
                <Toggle label="Repeatable" checked={!!q.repeatable} onChange={(v) => patch(i, { repeatable: v || undefined })} />
              </div>
            </div>
            <div className="pt-1">
              <p className="lbl">Rewards</p>
              {rewards.map((r, k) => (
                <div key={k} className="flex items-end gap-2">
                  <SelectField label="Item" value={r.item} options={withValue(items, r.item)} onChange={(v) => setReward(k, { item: v })} className="flex-1" />
                  <NumField label="How many" value={r.count} onCommit={(n) => setReward(k, { count: Math.min(255, Math.max(1, Math.round(n))) })} className="w-20" />
                  <IconButton label={`Remove reward ${r.item}`} onClick={() => patch(i, { reward: rewards.filter((_, j) => j !== k) })}>
                    <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                  </IconButton>
                </div>
              ))}
              <button
                type="button"
                className="btn mt-1"
                disabled={items.length === 0 || rewards.length >= MAX_QUEST_REWARDS}
                onClick={() => patch(i, { reward: [...rewards, { item: items[0]?.value ?? '', count: 1 }] })}
              >
                <Plus size={16} strokeWidth={1.5} aria-hidden />
                Add reward
              </button>
            </div>
          </Strip>
        )
      })}
      {problems.length > 0 && (
        <ul role="status" className="m-0 pt-2 pl-4 text-fg-3">
          {problems.map((p, k) => (
            <li key={k}>{p}</li>
          ))}
        </ul>
      )}
      {fresh ? (
        <AddButton label="Add quest" onClick={() => quests.length < MAX_QUESTS && setGame({ ...game, quests: [...quests, fresh] })} />
      ) : (
        <p className="m-0 pt-2 text-fg-3">Add a bot type first: every quest is given by one.</p>
      )}
      <p className="m-0 pt-3 text-label text-fg-3 max-w-[40rem]">
        Steps name bot types, places and items by name; renaming one here or in their tabs keeps the quests right, and removing one drops the steps (or the
        quest, for its giver) that needed it.
      </p>
    </div>
  )
}

// ------------------------------------------------------------------- classes

const OTHER_LOOK = '*other'

export function ClassesEditor({ game, setGame }: Edit) {
  const classes = game.classes ?? []
  // Every edit goes through cleanClass, so a field cleared or set back to zero is left out.
  const patch = (i: number, p: Partial<ClassFile>) => setGame({ ...game, classes: classes.map((c, k) => (k === i ? cleanClass({ ...c, ...p }) : c)) })
  const dup = (name: string, i: number) => classes.some((c, k) => k !== i && c.name === name)
  const share = (n: number) => Math.min(2, Math.max(0, n))
  // The first action is the plain attack every class has; the rest are what a class may be limited to.
  const spells = game.actions.slice(1).map((a) => a.name)
  const problems = classProblems(game)
  return (
    <div>
      <p className="text-fg-3 pb-1">
        Classes apply live: nobody is disconnected. A player picks one the first time they come into the game and keeps it; its might and toughness add to their
        level&apos;s and gear&apos;s.
      </p>
      {classes.length === 0 && <Empty>No classes. Without any, players just play; add one and new players choose from them when they come in.</Empty>}
      {classes.map((c, i) => {
        const look = c.look ?? ''
        const known = look === '' || (CLASS_LOOKS as readonly string[]).includes(look)
        const chosen = c.spells ?? []
        // A spell the class names that the game no longer has still shows, so it can be unticked (the problem list says so).
        const listed = [...spells, ...chosen.filter((n) => !spells.includes(n))]
        const toggle = (n: string, on: boolean) => patch(i, { spells: on ? listed.filter((x) => x === n || chosen.includes(x)) : chosen.filter((x) => x !== n) })
        return (
          <Strip key={i}>
            <div className="flex items-end gap-2">
              <TextField label="Class" value={c.name} invalid={dup(c.name, i)} maxLength={32} onCommit={(n) => setGame(renameClass(game, i, n.trim()))} className="w-48" />
              <TextField label="Title (players see this)" value={c.title} maxLength={32} onCommit={(t) => patch(i, { title: t.slice(0, 32) })} className="flex-1" />
              <IconButton label={`Delete class ${c.name}`} onClick={() => setGame(removeClass(game, i))}>
                <Trash2 size={16} strokeWidth={1.5} aria-hidden />
              </IconButton>
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-2">
              <SelectField
                label="Look"
                value={known ? look : OTHER_LOOK}
                options={[{ value: '', label: 'default' }, ...CLASS_LOOKS.map((l) => ({ value: l, label: l })), { value: OTHER_LOOK, label: 'other…' }]}
                onChange={(v) => patch(i, { look: v === OTHER_LOOK ? (known ? 'custom' : look) : v })}
              />
              {known ? <div /> : <TextField label="Model key" value={look} maxLength={32} onCommit={(t) => patch(i, { look: t.trim() })} />}
              <NumField label="Might (0 to 2)" value={c.might ?? 0} step={0.05} onCommit={(n) => patch(i, { might: share(n) })} />
              <NumField label="Toughness (0 to 2)" value={c.toughness ?? 0} step={0.05} onCommit={(n) => patch(i, { toughness: share(n) })} />
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-1">
              <div className="col-span-4">
                <TextField label="Description" value={c.desc ?? ''} allowEmpty maxLength={300} onCommit={(t) => patch(i, { desc: t.slice(0, 300) })} />
              </div>
            </div>
            <div className="pt-1">
              <p className="lbl">Spells {chosen.length === 0 ? '(none ticked: every action)' : ''}</p>
              {listed.length === 0 ? (
                <p className="m-0 text-fg-3">The game has no actions besides {game.actions[0]?.name ?? 'the plain attack'}, which every class has.</p>
              ) : (
                <div className="flex flex-wrap gap-x-4 gap-y-1">
                  {listed.map((n) => (
                    <label key={n} className="flex items-center gap-1 cursor-pointer">
                      <input type="checkbox" checked={chosen.includes(n)} onChange={(e) => toggle(n, e.target.checked)} className="accent-[var(--color-accent)]" />
                      <span className={spells.includes(n) ? '' : 'text-fg-3'}>{spells.includes(n) ? n : `${n} (missing)`}</span>
                    </label>
                  ))}
                </div>
              )}
            </div>
          </Strip>
        )
      })}
      {problems.length > 0 && (
        <ul role="status" className="m-0 pt-2 pl-4 text-fg-3">
          {problems.map((p, k) => (
            <li key={k}>{p}</li>
          ))}
        </ul>
      )}
      {classes.length < MAX_CLASSES && <AddButton label="Add class" onClick={() => setGame({ ...game, classes: [...classes, newClass(game)] })} />}
      <p className="m-0 pt-3 text-label text-fg-3 max-w-[40rem]">
        Spells name actions; renaming one in Actions keeps the classes right, and removing one unticks it. Every class may always use the first action, the plain
        attack. The reserved attribute “class” (make it replicated, 0 to 15) shows each player&apos;s class to the clients.
      </p>
    </div>
  )
}

// ---------------------------------------------------------------------- bots

export function BotsEditor({
  game,
  setGame,
  liveCounts,
  onSpawn,
  onKillAll,
}: Edit & {
  liveCounts: number[]
  onSpawn: (name: string, n: number) => void
  onKillAll: () => void
}) {
  const patch = (i: number, p: Partial<Archetype>) =>
    setGame({ ...game, archetypes: game.archetypes.map((a, k) => (k === i ? { ...a, ...p } : a)) })
  const dup = (name: string, i: number) => game.archetypes.some((a, k) => k !== i && a.name === name)
  return (
    <div>
      {game.archetypes.length === 0 && <Empty>No bot types yet. A bot is an ordinary entity with a behaviour and its own starting attributes.</Empty>}
      {game.archetypes.map((a, i) => {
        const free = game.attrs.filter((x) => !(x.name in a.attrs))
        return (
          <Strip key={i}>
            <div className="flex items-end gap-2">
              <label className="block">
                <span className="lbl block">Colour</span>
                <input
                  type="color"
                  value={a.color}
                  aria-label={`Colour of ${a.name}`}
                  onChange={(e) => patch(i, { color: e.target.value })}
                  className="block h-6 w-8 cursor-pointer border border-line bg-transparent p-0 rounded-sm"
                />
              </label>
              <TextField label="Bot type" value={a.name} invalid={dup(a.name, i)} onCommit={(n) => setGame(renameArch(game, i, n))} className="flex-1" />
              <TextField label="Shown as" value={a.title ?? ''} onCommit={(t) => patch(i, { title: t.trim().slice(0, 32) || undefined })} className="w-32" />
              <TextField
                label="Given names"
                value={(a.names ?? []).join(', ')}
                onCommit={(t) => {
                  const names = t.split(',').map((n) => n.trim().replace(/\|/g, '').slice(0, 20)).filter(Boolean).slice(0, 200)
                  patch(i, { names: names.length ? names : undefined })
                }}
                className="w-48"
              />
              <IconButton
                label={`Delete bot type ${a.name}`}
                onClick={() => setGame(removeArch(game, i))}
              >
                <Trash2 size={16} strokeWidth={1.5} aria-hidden />
              </IconButton>
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-2">
              <div className="min-w-0">
                <SelectField
                  label="Behaviour"
                  value={a.behaviour}
                  options={[
                    { value: 'idle', label: 'idle' },
                    { value: 'wander', label: 'wander' },
                    { value: 'chase', label: 'chase' },
                    { value: 'flee', label: 'flee' },
                  ]}
                  onChange={(v) => patch(i, { behaviour: v })}
                />
              </div>
              <NumField label="Speed ×" value={a.speed_mult} onCommit={(n) => patch(i, { speed_mult: n })} />
              <NumField label="Sight" value={a.sight} onCommit={(n) => patch(i, { sight: n })} />
              <NumField label="Spawn count" value={a.count} onCommit={(n) => patch(i, { count: Math.max(0, Math.round(n)) })} />
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-1">
              <NumField label="Experience" value={a.xp ?? 0} onCommit={(n) => patch(i, { xp: Math.max(0, Math.round(n)) || undefined })} />
              <NumField label="Lasts (s, 0 for ever)" value={a.lasts_secs ?? 0} onCommit={(n) => patch(i, { lasts_secs: Math.max(0, Math.round(n)) || undefined })} />
              <NumField label="Level" value={a.level ?? 1} onCommit={(n) => patch(i, { level: Math.min(100, Math.max(1, Math.round(n))) > 1 ? Math.min(100, Math.round(n)) : undefined })} />
              <NumField label="Gives up at (0 auto)" value={a.leash ?? 0} onCommit={(n) => patch(i, { leash: Math.min(10_000, Math.max(0, n)) || undefined })} />
              <NumField label="Back after (s)" value={a.respawn_secs ?? 0} onCommit={(n) => patch(i, { respawn_secs: Math.min(86_400, Math.max(0, n)) || undefined })} />
              <SelectField
                label="Comes back"
                value={a.respawn_at ?? 'anywhere'}
                options={[{ value: 'anywhere', label: 'anywhere' }, { value: 'home', label: 'at home' }, { value: 'here', label: 'where it fell' }]}
                onChange={(v) => patch(i, { respawn_at: v === 'anywhere' ? undefined : (v as 'home' | 'here') })}
              />
              {(game.factions ?? []).length > 0 && (
                <SelectField
                  label="Faction"
                  value={a.faction ?? ''}
                  options={[{ value: '', label: 'none' }, ...(game.factions ?? []).map((f) => ({ value: f.name, label: f.title || f.name }))]}
                  onChange={(v) => patch(i, { faction: v === '' ? undefined : v })}
                />
              )}
            </div>
            <div className="grid grid-cols-4 gap-x-2 pt-1">
              <SelectField
                label="Attacks with"
                value={a.attack ?? ''}
                options={[{ value: '', label: 'nothing' }, ...game.actions.map((x) => ({ value: x.name, label: x.name }))]}
                onChange={(v) => patch(i, { attack: v === '' ? null : v })}
                className="col-span-2"
              />
              <div className="col-span-2 flex items-end gap-1 text-fg-2">
                <span className="flex items-center gap-1 h-6" title={a.behaviour}>
                  {BEHAVIOUR_ICON[a.behaviour]}
                </span>
                <span className="h-6 truncate" aria-label={`${liveCounts[i] ?? 0} alive`}>
                  {liveCounts[i] ?? 0} alive
                </span>
              </div>
            </div>
            <div className="pt-1">
              <p className="lbl">Starting attributes that differ</p>
              {Object.entries(a.attrs).map(([k, v]) => (
                <div key={k} className="flex items-end gap-2">
                  <span className="flex-1 h-6 truncate border-b border-line">{k}</span>
                  <NumField
                    label="Value"
                    value={v}
                    onCommit={(n) => patch(i, { attrs: { ...a.attrs, [k]: n } })}
                    className="w-24"
                  />
                  <IconButton
                    label={`Remove override ${k}`}
                    onClick={() => patch(i, { attrs: Object.fromEntries(Object.entries(a.attrs).filter(([x]) => x !== k)) })}
                  >
                    <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                  </IconButton>
                </div>
              ))}
              {free.length > 0 && (
                <label className="block pt-1">
                  <span className="sr-only">Override a starting attribute for {a.name}</span>
                  <select
                    className="field cursor-pointer text-fg-3"
                    value=""
                    onChange={(e) => {
                      const at = game.attrs.find((x) => x.name === e.target.value)
                      if (at) patch(i, { attrs: { ...a.attrs, [at.name]: at.default } })
                    }}
                  >
                    <option value="" className="bg-bg">
                      + override an attribute
                    </option>
                    {free.map((x) => (
                      <option key={x.name} value={x.name} className="bg-bg text-fg">
                        {x.name}
                      </option>
                    ))}
                  </select>
                </label>
              )}
            </div>
            <div className="pt-1">
              <p className="lbl">Leaves behind</p>
              {(a.drops ?? []).map((d, k) => {
                const setDrop = (p: Partial<typeof d>) => patch(i, { drops: (a.drops ?? []).map((x, j) => (j === k ? { ...x, ...p } : x)) })
                return (
                  <div key={k} className="flex items-end gap-2">
                    <SelectField label="Item" value={d.item} options={(game.items ?? []).map((x) => ({ value: x.name, label: x.name }))} onChange={(v) => setDrop({ item: v })} className="flex-1" />
                    <NumField label="Chance %" value={d.chance_pct} onCommit={(n) => setDrop({ chance_pct: Math.max(0, Math.min(100, n)) })} className="w-20" />
                    <NumField label="Min" value={d.min} onCommit={(n) => setDrop({ min: Math.max(1, Math.min(99, Math.round(n))), max: Math.max(d.max, Math.round(n)) })} className="w-14" />
                    <NumField label="Max" value={d.max} onCommit={(n) => setDrop({ max: Math.max(d.min, Math.min(99, Math.round(n))) })} className="w-14" />
                    <IconButton label={`Remove drop of ${d.item}`} onClick={() => patch(i, { drops: (a.drops ?? []).filter((_, j) => j !== k) })}>
                      <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                    </IconButton>
                  </div>
                )
              })}
              <button
                type="button"
                className="btn mt-1"
                disabled={(game.items ?? []).length === 0 || (a.drops ?? []).length >= 8}
                onClick={() => patch(i, { drops: [...(a.drops ?? []), { item: game.items?.[0]?.name ?? '', chance_pct: 50, min: 1, max: 1 }] })}
              >
                <Plus size={16} strokeWidth={1.5} aria-hidden />
                Add drop
              </button>
            </div>
            <div className="pt-1">
              <p className="lbl">Sells{game.currency ? ` (for ${game.currency})` : ''}</p>
              {(a.shop ?? []).map((s, k) => {
                const setShop = (p: Partial<typeof s>) => patch(i, { shop: (a.shop ?? []).map((x, j) => (j === k ? { ...x, ...p } : x)) })
                return (
                  <div key={k} className="flex items-end gap-2">
                    <SelectField label="Item" value={s.item} options={(game.items ?? []).map((x) => ({ value: x.name, label: x.name }))} onChange={(v) => setShop({ item: v })} className="flex-1" />
                    <NumField label="Price" value={s.price} onCommit={(n) => setShop({ price: Math.max(0, Math.min(65535, Math.round(n))) })} className="w-20" />
                    <IconButton
                      label={`Stop selling ${s.item}`}
                      onClick={() => {
                        const shop = (a.shop ?? []).filter((_, j) => j !== k)
                        patch(i, { shop: shop.length > 0 ? shop : undefined })
                      }}
                    >
                      <Trash2 size={16} strokeWidth={1.5} aria-hidden />
                    </IconButton>
                  </div>
                )
              })}
              <button
                type="button"
                className="btn mt-1"
                disabled={(game.items ?? []).length === 0}
                onClick={() => patch(i, { shop: [...(a.shop ?? []), { item: game.items?.[0]?.name ?? '', price: 10 }] })}
              >
                <Plus size={16} strokeWidth={1.5} aria-hidden />
                Add to shop
              </button>
            </div>
            <div className="flex gap-2 pt-2">
              <button type="button" className="btn" onClick={() => onSpawn(a.name, 1)}>
                Spawn 1
              </button>
              <button type="button" className="btn" onClick={() => onSpawn(a.name, 10)}>
                Spawn 10
              </button>
              <button type="button" className="btn" onClick={() => void navigator.clipboard?.writeText(exportArchetype(a))} aria-label={`Copy ${a.name} as JSON`}>
                <Copy size={16} strokeWidth={1.5} aria-hidden />
                Copy JSON
              </button>
            </div>
          </Strip>
        )
      })}
      <div className="flex gap-2 mt-2">
        <AddButton label="Add bot type" onClick={() => setGame({ ...game, archetypes: [...game.archetypes, newArchetype(game)] })} />
        <button type="button" className="btn mt-2" onClick={onKillAll}>
          <Trash2 size={16} strokeWidth={1.5} aria-hidden />
          Remove all live bots
        </button>
      </div>
      <QuickAdd game={game} setGame={setGame} />
    </div>
  )
}

/** The fast ways to get a new bot: a template, pasted JSON, or a file in the server's bots folder. */
function QuickAdd({ game, setGame }: Edit) {
  const [text, setText] = useState('')
  const [note, setNote] = useState<string | null>(null)
  return (
    <div className="pt-4 grid gap-2 max-w-[40rem]">
      <label className="block">
        <span className="lbl">Start from a template</span>
        <select
          className="field cursor-pointer"
          value=""
          onChange={(e) => {
            if (e.target.value) setGame(addFromTemplate(game, e.target.value))
          }}
        >
          <option value="" className="bg-bg">
            choose one…
          </option>
          {BOT_TEMPLATES.map((t) => (
            <option key={t.id} value={t.id} className="bg-bg text-fg">
              {t.label}
            </option>
          ))}
        </select>
      </label>
      <label className="block">
        <span className="lbl">Or paste a bot type as JSON</span>
        <textarea
          className="field !h-24 py-1 font-mono"
          spellCheck={false}
          value={text}
          onChange={(e) => {
            setText(e.target.value)
            setNote(null)
          }}
          placeholder='{"name": "warden", "behaviour": "wander", "count": 4}'
        />
      </label>
      <div className="flex items-center gap-3">
        <button
          type="button"
          className="btn"
          disabled={!text.trim()}
          onClick={() => {
            const r = importArchetypes(game, text)
            if ('error' in r) return setNote(r.error)
            setGame(r.game)
            setText('')
            setNote(`Added ${r.names.join(', ')}. Press Apply to start them.`)
          }}
        >
          Add from JSON
        </button>
        {note && (
          <span role="status" className="text-fg-3">
            {note}
          </span>
        )}
      </div>
      <p className="m-0 text-label text-fg-3">
        No console needed either: drop a <code>.json</code> file in the server's <code>bots.d</code> folder and the bot joins the running world within two
        seconds, with nobody disconnected. “Copy JSON” on any bot above gives you the file's contents.
      </p>
    </div>
  )
}
