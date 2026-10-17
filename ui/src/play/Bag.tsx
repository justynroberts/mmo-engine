import { useEffect, useState, type MutableRefObject } from 'react'
import type { GameClient } from './client'
import { ACT_DROP, ACT_PICKUP, ACT_USE, BAG_SLOTS, PICKUP_RANGE, type Shared } from './shared'

interface Props {
  client: GameClient
  shared: MutableRefObject<Shared>
  /** Called after the player asked for something, for a sound. */
  onAct: (what: 'use' | 'drop' | 'pickup') => void
  /** True while the drop key is armed: the next number drops instead of uses. */
  dropArmed: boolean
}

interface View {
  slots: { name: string; color: string; count: number; usable: boolean }[]
  near: { name: string; color: string; count: number } | null
}

/** Your eight slots, and a prompt when something you can pick up is within reach. */
export default function Bag({ client, shared, onAct, dropArmed }: Props) {
  const [v, setV] = useState<View>({ slots: [], near: null })
  useEffect(() => {
    const t = setInterval(() => {
      const items = shared.current.items
      const slots = Array.from({ length: BAG_SLOTS }, (_, i) => {
        const s = client.bag[i]
        const it = s && s.item >= 0 ? items[s.item] : undefined
        return it && s ? { name: it.name, color: it.color, count: s.count, usable: it.usable } : { name: '', color: '', count: 0, usable: false }
      })
      const me = client.last.self
      let near: View['near'] = null
      if (me) {
        let best = PICKUP_RANGE * PICKUP_RANGE
        for (const g of client.ground) {
          const d = (g.x - me.x) ** 2 + (g.y - me.y) ** 2
          const it = items[g.item]
          if (d <= best && it) {
            best = d
            near = { name: it.name, color: it.color, count: g.count }
          }
        }
      }
      setV((p) => (JSON.stringify(p) === JSON.stringify({ slots, near }) ? p : { slots, near }))
    }, 100)
    return () => clearInterval(t)
  }, [client, shared])

  const any = v.slots.some((s) => s.count > 0)
  if (!any && !v.near && shared.current.items.length === 0) return null
  return (
    <div className="absolute left-1/2 top-3 -translate-x-1/2 grid justify-items-center gap-1 pointer-events-none max-[760px]:top-[3.4rem]">
      {v.near && (
        <button
          type="button"
          className="hud-btn pointer-events-auto"
          onClick={() => {
            client.act(ACT_PICKUP, 0)
            onAct('pickup')
          }}
          aria-label={`Pick up ${v.near.count} ${v.near.name} (E)`}
        >
          <Gem color={v.near.color} />
          pick up {v.near.name}
          {v.near.count > 1 ? ` ×${v.near.count}` : ''}
          <span className="hud-dim">E</span>
        </button>
      )}
      <ul className="list-none m-0 p-0 flex gap-1" aria-label={dropArmed ? 'Bag, choose a slot to drop' : 'Bag'}>
        {v.slots.map((s, i) => (
          <li key={i} className="grid gap-px">
            <button
              type="button"
              disabled={s.count === 0}
              className={`hud-btn pointer-events-auto relative h-10 w-10 !px-0 ${dropArmed && s.count > 0 ? '!border-[color:var(--hud-hot)]' : ''}`}
              aria-label={s.count ? `${s.name} ×${s.count}, ${s.usable ? 'use' : 'cannot be used'} (Shift ${i + 1})` : `Empty slot ${i + 1}`}
              title={s.count ? `${s.name}${s.usable ? '' : ' (just carried)'}` : undefined}
              onClick={() => {
                if (!s.count) return
                client.act(dropArmed ? ACT_DROP + i : ACT_USE + i, 0)
                onAct(dropArmed ? 'drop' : 'use')
              }}
            >
              <span className="absolute left-1 top-0.5 text-[9px] hud-dim leading-none" aria-hidden>
                {i + 1}
              </span>
              {s.count > 0 && <Gem color={s.color} big />}
              {s.count > 1 && (
                <span className="absolute right-1 bottom-0.5 text-[10px] leading-none" aria-hidden>
                  {s.count}
                </span>
              )}
            </button>
            {s.count > 0 && (
              <button
                type="button"
                className="pointer-events-auto text-[10px] leading-4 hud-dim bg-transparent border-0 cursor-pointer hover:text-[color:var(--hud-hot)]"
                aria-label={`Drop ${s.name}`}
                onClick={() => {
                  client.act(ACT_DROP + i, 0)
                  onAct('drop')
                }}
              >
                drop
              </button>
            )}
          </li>
        ))}
      </ul>
    </div>
  )
}

/** A small lit diamond in an item's colour. */
function Gem({ color, big = false }: { color: string; big?: boolean }) {
  const s = big ? 14 : 10
  return <span aria-hidden className="inline-block rotate-45" style={{ width: s, height: s, background: color, boxShadow: `0 0 ${big ? 10 : 6}px ${color}` }} />
}
