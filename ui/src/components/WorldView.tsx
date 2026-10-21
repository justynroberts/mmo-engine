import { kindOf } from '../play/blocks'
import { useEffect, useRef, type MutableRefObject } from 'react'
import { KIND_DRIVEN, KIND_GHOST, KIND_PLAYER, type Place, type Route, type State, type WorldGeom } from '../api'
import type { Sel, Tool } from './Builder'
import { BLOCK, BLOCK_COLOURS, MAP, OPEN, TREES, TYPE_COLOUR, groundImage, hpColor, type BlockCol } from './mapLook'
import { fromBoxes, type WorldData, type WorldEdit } from '../world'

/** When present the map is in builder mode: it shows places and routes and edits them. */
export interface EditOverlay {
  places: Place[]
  routes: Route[]
  home: string | null
  sel: Sel
  tool: Tool
  draft: [number, number][]
  /** Hand-placed walls being edited (null while the world's own walls are in use). */
  walls: [number, number, number, number][] | null
  onAddWall: (x0: number, y0: number, x1: number, y1: number) => void
  onPick: (s: Sel) => void
  onClick: (x: number, y: number) => void
  onMovePlace: (i: number, x: number, y: number) => void
  onMovePoint: (route: number, point: number, x: number, y: number) => void
}

/** When present the map shapes the world itself (the World tab): remove what is at a point, or drag out a footprint. */
export interface ShapeOverlay {
  tool: 'pan' | 'remove' | 'add'
  edits: WorldEdit[]
  /** True while the map shows a preview rather than the live world. */
  previewing: boolean
  onRemove: (x: number, y: number) => void
  onAdd: (x0: number, y0: number, x1: number, y1: number) => void
}

interface Props {
  edit?: EditOverlay | null
  shape?: ShapeOverlay | null
  world: WorldGeom | null
  stateRef: MutableRefObject<State | null>
  colors: string[]
  selected: number | null
  onSelect: (id: number | null) => void
  follow: boolean
  interest: number
  healthIdx: number
  healthMax: number
  themeKey: string
  /** The world file, when the server has one: heights, water and objects to draw. Without it, the walls. */
  land?: WorldData | null
  /** Blocks players have built. */
  blocks?: BlockCol[]
}

interface View {
  cx: number
  cy: number
  scale: number
  /** World size the view was last fitted to (0 = refit on next frame). */
  fittedFor: number
}


export function WorldView(p: Props) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const box = useRef<HTMLDivElement>(null)
  const view = useRef<View>({ cx: 0, cy: 0, scale: 1, fittedFor: 0 })
  const size = useRef({ w: 300, h: 300 })
  const mouse = useRef<{ x: number; y: number } | null>(null)
  /** Rectangle being dragged with the wall tool, in world units. */
  const wallDrag = useRef<{ x0: number; y0: number; x1: number; y1: number } | null>(null)
  const props = useRef(p)
  props.current = p

  // Keep the backing store matched to the element at the device pixel ratio.
  useEffect(() => {
    const el = box.current
    const cv = canvas.current
    if (!el || !cv) return
    const fit = () => {
      const r = el.getBoundingClientRect()
      const dpr = window.devicePixelRatio || 1
      size.current = { w: Math.max(50, r.width), h: Math.max(50, r.height) }
      cv.width = Math.round(size.current.w * dpr)
      cv.height = Math.round(size.current.h * dpr)
      view.current.fittedFor = 0
    }
    fit()
    const ro = new ResizeObserver(fit)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // Interaction: drag to pan, wheel to zoom at the cursor, click to select.
  useEffect(() => {
    const cv = canvas.current
    if (!cv) return
    const toWorld = (px: number, py: number) => {
      const v = view.current
      return { x: (px - size.current.w / 2) / v.scale + v.cx, y: (py - size.current.h / 2) / v.scale + v.cy }
    }
    let drag: { x: number; y: number; cx: number; cy: number; moved: boolean } | null = null
    // A handle being dragged in builder mode instead of the view.
    let grab: { kind: 'place'; i: number } | { kind: 'point'; r: number; k: number } | null = null
    const toScreen = (x: number, y: number) => {
      const v = view.current
      return { x: (x - v.cx) * v.scale + size.current.w / 2, y: (y - v.cy) * v.scale + size.current.h / 2 }
    }
    /** What editable thing is under a screen point: route points first, then places. */
    const hitEdit = (l: { x: number; y: number }) => {
      const ed = props.current.edit
      if (!ed) return null
      let best: { kind: 'point'; r: number; k: number; d: number } | null = null
      ed.routes.forEach((r, ri) =>
        r.points.forEach((pt, k) => {
          const sp = toScreen(pt[0], pt[1])
          const d = Math.hypot(sp.x - l.x, sp.y - l.y)
          if (d <= 9 && (!best || d < best.d)) best = { kind: 'point', r: ri, k, d }
        }),
      )
      if (best) return best as { kind: 'point'; r: number; k: number }
      let pb: { kind: 'place'; i: number; d: number } | null = null
      ed.places.forEach((p, i) => {
        const sp = toScreen(p.x, p.y)
        const d = Math.hypot(sp.x - l.x, sp.y - l.y)
        if ((d <= 10 || d <= p.r * view.current.scale) && (!pb || d < pb.d)) pb = { kind: 'place', i, d }
      })
      return pb as { kind: 'place'; i: number } | null
    }
    const worldOf = (l: { x: number; y: number }) => {
      const v = view.current
      const max = props.current.world?.size ?? 2048
      return {
        x: Math.min(max, Math.max(0, (l.x - size.current.w / 2) / v.scale + v.cx)),
        y: Math.min(max, Math.max(0, (l.y - size.current.h / 2) / v.scale + v.cy)),
      }
    }
    const local = (e: PointerEvent | WheelEvent) => {
      const r = cv.getBoundingClientRect()
      return { x: e.clientX - r.left, y: e.clientY - r.top }
    }
    const down = (e: PointerEvent) => {
      const l = local(e)
      const ed = props.current.edit
      if (ed && ed.tool === 'select') {
        const h = hitEdit(l)
        if (h) {
          grab = h
          ed.onPick(h.kind === 'place' ? { kind: 'place', index: h.i } : { kind: 'route', index: h.r })
          cv.setPointerCapture(e.pointerId)
          return
        }
      }
      if ((ed && ed.tool === 'wall') || props.current.shape?.tool === 'add') {
        const w = worldOf(l)
        wallDrag.current = { x0: w.x, y0: w.y, x1: w.x, y1: w.y }
        cv.setPointerCapture(e.pointerId)
        return
      }
      drag = { x: l.x, y: l.y, cx: view.current.cx, cy: view.current.cy, moved: false }
      cv.setPointerCapture(e.pointerId)
    }
    const move = (e: PointerEvent) => {
      const l = local(e)
      mouse.current = l
      if (wallDrag.current) {
        const w = worldOf(l)
        wallDrag.current.x1 = w.x
        wallDrag.current.y1 = w.y
        return
      }
      if (grab) {
        const w = worldOf(l)
        const ed = props.current.edit
        if (ed && grab.kind === 'place') ed.onMovePlace(grab.i, Math.round(w.x), Math.round(w.y))
        else if (ed && grab.kind === 'point') ed.onMovePoint(grab.r, grab.k, Math.round(w.x), Math.round(w.y))
        return
      }
      if (!drag) return
      const dx = l.x - drag.x
      const dy = l.y - drag.y
      if (Math.abs(dx) + Math.abs(dy) > 4) drag.moved = true
      if (drag.moved) {
        view.current.cx = drag.cx - dx / view.current.scale
        view.current.cy = drag.cy - dy / view.current.scale
      }
    }
    const up = (e: PointerEvent) => {
      const l = local(e)
      if (wallDrag.current) {
        const r = wallDrag.current
        wallDrag.current = null
        cv.releasePointerCapture(e.pointerId)
        const sh = props.current.shape
        if (sh?.tool === 'add') sh.onAdd(r.x0, r.y0, r.x1, r.y1)
        else props.current.edit?.onAddWall(r.x0, r.y0, r.x1, r.y1)
        return
      }
      if (grab) {
        grab = null
        cv.releasePointerCapture(e.pointerId)
        return
      }
      const ed = props.current.edit
      const sh = props.current.shape
      if (drag && !drag.moved && sh?.tool === 'remove') {
        const w = worldOf(l)
        sh.onRemove(w.x, w.y)
      } else if (drag && !drag.moved && ed) {
        if (ed.tool === 'select') {
          // A click on a hand-placed wall selects it; anywhere else clears the selection.
          const w = worldOf(l)
          const i = (ed.walls ?? []).findIndex((q) => w.x >= q[0] && w.x <= q[2] && w.y >= q[1] && w.y <= q[3])
          ed.onPick(i >= 0 ? { kind: 'wall', index: i } : null)
        } else if (ed.tool === 'wall') {
          // handled on pointer up above
        } else {
          const w = worldOf(l)
          ed.onClick(w.x, w.y)
        }
      } else if (drag && !drag.moved) {
        const s = props.current.stateRef.current
        const v = view.current
        let best: number | null = null
        let bd = 12 * 12
        if (s) {
          for (const row of s.e) {
            const sx = ((row[1] ?? 0) - v.cx) * v.scale + size.current.w / 2
            const sy = ((row[2] ?? 0) - v.cy) * v.scale + size.current.h / 2
            const d = (sx - l.x) ** 2 + (sy - l.y) ** 2
            if (d < bd) {
              bd = d
              best = row[0] ?? null
            }
          }
        }
        props.current.onSelect(best)
      }
      drag = null
      cv.releasePointerCapture(e.pointerId)
    }
    const leave = () => (mouse.current = null)
    const wheel = (e: WheelEvent) => {
      e.preventDefault()
      const l = local(e)
      const before = toWorld(l.x, l.y)
      const k = Math.exp(-e.deltaY * 0.0015)
      view.current.scale = Math.min(60, Math.max(0.05, view.current.scale * k))
      const after = toWorld(l.x, l.y)
      view.current.cx += before.x - after.x
      view.current.cy += before.y - after.y
    }
    cv.addEventListener('pointerdown', down)
    cv.addEventListener('pointermove', move)
    cv.addEventListener('pointerup', up)
    cv.addEventListener('pointerleave', leave)
    cv.addEventListener('wheel', wheel, { passive: false })
    return () => {
      cv.removeEventListener('pointerdown', down)
      cv.removeEventListener('pointermove', move)
      cv.removeEventListener('pointerup', up)
      cv.removeEventListener('pointerleave', leave)
      cv.removeEventListener('wheel', wheel)
    }
  }, [])

  // Draw loop.
  useEffect(() => {
    let raf = 0
    // The ground picture and the trees only change with the world, so they are made once per world.
    let look: { land: WorldData; ground: HTMLCanvasElement | null } | null = null
    const draw = () => {
      raf = requestAnimationFrame(draw)
      const cv = canvas.current
      const ctx = cv?.getContext('2d')
      const pr = props.current
      if (!cv || !ctx) return
      const { w, h } = size.current
      const v = view.current
      const worldSize = pr.world?.size ?? 2048
      // Fit on first sight of the world, after a resize, and when the world size changes.
      if (pr.world && v.fittedFor !== worldSize) {
        v.scale = Math.min(w, h) / worldSize
        v.cx = worldSize / 2
        v.cy = worldSize / 2
        v.fittedFor = worldSize
      }
      const dpr = window.devicePixelRatio || 1
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      // The world file if there is one, else the server's walls as plain blocks on flat ground.
      const land = pr.land ?? (pr.world ? fromBoxes(worldSize, pr.world.boxes) : null)
      if (land && look?.land !== land) look = { land, ground: land.hasHeights ? groundImage(land) : null }
      const fg = MAP.parchment
      const accent = MAP.gold
      ctx.fillStyle = MAP.void
      ctx.fillRect(0, 0, w, h)

      const sx = (x: number) => (x - v.cx) * v.scale + w / 2
      const sy = (y: number) => (y - v.cy) * v.scale + h / 2
      if (look?.ground) {
        ctx.imageSmoothingEnabled = true
        ctx.drawImage(look.ground, sx(0), sy(0), worldSize * v.scale, worldSize * v.scale)
      } else {
        ctx.fillStyle = '#5f8c3a'
        ctx.fillRect(sx(0), sy(0), worldSize * v.scale, worldSize * v.scale)
      }

      // Open ground first (a city's streets, squares), then water, then everything that stands.
      const objs = look?.land.objects ?? []
      const onScreen = (b: number[]) => {
        const x = sx(b[0]!)
        const y = sy(b[1]!)
        return !(x > w || y > h || x + (b[2]! - b[0]!) * v.scale < 0 || y + (b[3]! - b[1]!) * v.scale < 0)
      }
      for (const o of objs) {
        if (!o.b || !OPEN.has(o.t) || !onScreen(o.b)) continue
        ctx.fillStyle = TYPE_COLOUR[o.t] ?? MAP.stone
        ctx.globalAlpha = 0.55
        ctx.fillRect(sx(o.b[0]), sy(o.b[1]), (o.b[2] - o.b[0]) * v.scale, (o.b[3] - o.b[1]) * v.scale)
        ctx.globalAlpha = 1
      }
      ctx.strokeStyle = MAP.water
      ctx.lineJoin = 'round'
      ctx.lineCap = 'round'
      for (const r of look?.land.rivers ?? []) {
        ctx.lineWidth = Math.max(1.5, r.half_width * 2 * v.scale)
        ctx.beginPath()
        r.pts.forEach((p, k) => (k === 0 ? ctx.moveTo(sx(p[0]), sy(p[1])) : ctx.lineTo(sx(p[0]), sy(p[1]))))
        ctx.stroke()
      }
      ctx.lineCap = 'butt'
      // Trees as canopies seen from above, once they are big enough to read.
      if (v.scale > 0.9) {
        const r = Math.max(1.5, v.scale * 2.2)
        ctx.fillStyle = MAP.tree
        ctx.strokeStyle = MAP.treeEdge
        ctx.lineWidth = 1
        for (const o of objs) {
          if (!o.p || !TREES.has(o.t)) continue
          const x = sx(o.p[0])
          const y = sy(o.p[1])
          if (x < -20 || y < -20 || x > w + 20 || y > h + 20) continue
          ctx.beginPath()
          ctx.arc(x, y, r * ((o.s ?? 1.6) / 3) * (o.t === 'bush' ? 0.6 : 1), 0, Math.PI * 2)
          ctx.fill()
          if (r > 3) ctx.stroke()
        }
      }

      // Grid every 64 units, only while it is readable.
      if (v.scale * 64 > 12) {
        ctx.strokeStyle = MAP.ink
        ctx.globalAlpha = 0.12
        ctx.lineWidth = 1
        ctx.beginPath()
        for (let g = 0; g <= worldSize; g += 64) {
          ctx.moveTo(Math.round(sx(g)) + 0.5, sy(0))
          ctx.lineTo(Math.round(sx(g)) + 0.5, sy(worldSize))
          ctx.moveTo(sx(0), Math.round(sy(g)) + 0.5)
          ctx.lineTo(sx(worldSize), Math.round(sy(g)) + 0.5)
        }
        ctx.stroke()
        ctx.globalAlpha = 1
      }
      ctx.strokeStyle = MAP.ink
      ctx.strokeRect(sx(0) + 0.5, sy(0) + 0.5, worldSize * v.scale, worldSize * v.scale)

      // Everything with a footprint, coloured by its type, with a darker rim and an inset line when there is room.
      for (const o of objs) {
        if (!o.b || OPEN.has(o.t) || !onScreen(o.b)) continue
        const x = sx(o.b[0])
        const y = sy(o.b[1])
        const bw = (o.b[2] - o.b[0]) * v.scale
        const bh = (o.b[3] - o.b[1]) * v.scale
        ctx.fillStyle = TYPE_COLOUR[o.t] ?? MAP.stone
        ctx.fillRect(x, y, bw, bh)
        if (o.t === 'lake') continue
        ctx.strokeStyle = MAP.stoneEdge
        ctx.lineWidth = 1
        ctx.strokeRect(Math.round(x) + 0.5, Math.round(y) + 0.5, Math.round(bw), Math.round(bh))
        const inset = v.scale * 1.2
        if (inset >= 2 && bw > inset * 4 && bh > inset * 4) {
          ctx.globalAlpha = 0.35
          ctx.strokeRect(x + inset, y + inset, bw - inset * 2, bh - inset * 2)
          ctx.globalAlpha = 1
        }
      }

      // Built blocks, coloured by the top one of each column, at least a pixel and a half so they show far out.
      if (pr.blocks?.length) {
        const bw = Math.max(1.5, BLOCK * v.scale)
        for (const [cx, cy, , kinds] of pr.blocks) {
          const x = sx(cx * BLOCK)
          const y = sy(cy * BLOCK)
          if (x < -4 || y < -4 || x > w + 4 || y > h + 4) continue
          ctx.fillStyle = BLOCK_COLOURS[Math.max(0, kindOf(kinds[kinds.length - 1] ?? ''))]! // the top layer is never empty
          ctx.fillRect(x, y, bw, bw)
        }
      }

      if (pr.edit) drawEdit(ctx, pr.edit, v, w, h, { fg, fg3: MAP.ink, accent, line: MAP.ink, bg: MAP.parchment }, mouse.current, wallDrag.current)
      if (pr.shape) drawShape(ctx, pr.shape, v, w, h, accent, wallDrag.current)

      const s = pr.stateRef.current
      let sel: (number[] | undefined)
      if (s) {
        const px = Math.min(9, Math.max(4, v.scale * 1.4))
        const showBars = v.scale > 1.2
        for (const row of s.e) {
          const x = sx(row[1] ?? 0)
          const y = sy(row[2] ?? 0)
          if (x < -10 || y < -10 || x > w + 10 || y > h + 10) continue
          const kind = row[4] ?? 0
          const arch = row[3] ?? -1
          const col = arch >= 0 ? pr.colors[arch] ?? MAP.parchment : MAP.parchment
          if (row[0] === pr.selected) sel = row
          // Every marker has an ink rim so it reads on grass, stone and dirt alike.
          ctx.lineWidth = 1
          ctx.strokeStyle = MAP.ink
          if (kind === KIND_PLAYER) {
            // Players are parchment diamonds; gold is kept for selection and possession.
            ctx.fillStyle = fg
            ctx.beginPath()
            ctx.moveTo(x, y - px * 0.8)
            ctx.lineTo(x + px * 0.8, y)
            ctx.lineTo(x, y + px * 0.8)
            ctx.lineTo(x - px * 0.8, y)
            ctx.closePath()
            ctx.fill()
            ctx.stroke()
          } else if (kind === KIND_DRIVEN) {
            ctx.fillStyle = col
            ctx.fillRect(x - px / 2, y - px / 2, px, px)
            ctx.strokeRect(x - px / 2 - 0.5, y - px / 2 - 0.5, px + 1, px + 1)
            ctx.strokeStyle = accent
            ctx.lineWidth = 1.5
            ctx.strokeRect(x - px / 2 - 2.5, y - px / 2 - 2.5, px + 5, px + 5)
          } else if (kind === KIND_GHOST) {
            // A mirror of something in the neighbouring zone: hollow and faint.
            ctx.strokeStyle = MAP.parchment
            ctx.strokeRect(x - px / 2 + 0.5, y - px / 2 + 0.5, px - 1, px - 1)
          } else {
            ctx.fillStyle = col
            ctx.fillRect(x - px / 2, y - px / 2, px, px)
            ctx.strokeRect(x - px / 2 - 0.5, y - px / 2 - 0.5, px + 1, px + 1)
          }
          if (showBars && pr.healthIdx >= 0) {
            const hp = (row[5 + pr.healthIdx] ?? 0) / (pr.healthMax || 1)
            if (hp < 0.999) {
              const f = Math.max(0, Math.min(1, hp))
              ctx.fillStyle = MAP.ink
              ctx.fillRect(x - px - 1, y + px / 2 + 2, px * 2 + 2, 4)
              ctx.fillStyle = hpColor(f)
              ctx.fillRect(x - px, y + px / 2 + 3, px * 2 * f, 2)
            }
          }
        }
        if (sel) {
          const x = sx(sel[1] ?? 0)
          const y = sy(sel[2] ?? 0)
          ctx.strokeStyle = accent
          ctx.lineWidth = 1
          ctx.setLineDash([3, 3])
          ctx.beginPath()
          ctx.arc(x, y, pr.interest * v.scale, 0, Math.PI * 2)
          ctx.stroke()
          ctx.setLineDash([])
          const r = 9
          ctx.lineWidth = 1.5
          ctx.beginPath()
          for (const [dx, dy] of [[-1, -1], [1, -1], [1, 1], [-1, 1]] as const) {
            ctx.moveTo(x + dx * r, y + dy * (r - 4))
            ctx.lineTo(x + dx * r, y + dy * r)
            ctx.lineTo(x + dx * (r - 4), y + dy * r)
          }
          ctx.stroke()
          if (pr.follow) {
            v.cx += ((sel[1] ?? 0) - v.cx) * 0.2
            v.cy += ((sel[2] ?? 0) - v.cy) * 0.2
          }
        }
      }

      // Readout, on an ink band so it reads over any ground.
      ctx.font = "12px 'Instrument Sans Variable', system-ui, sans-serif"
      ctx.fillStyle = MAP.ink
      ctx.globalAlpha = 0.72
      ctx.fillRect(0, h - 22, w, 22)
      ctx.globalAlpha = 1
      ctx.fillStyle = MAP.parchment
      const m = mouse.current
      const wx = m ? (m.x - w / 2) / v.scale + v.cx : null
      const wy = m ? (m.y - h / 2) / v.scale + v.cy : null
      ctx.fillText(
        `zoom ${v.scale.toFixed(2)} px/u${wx !== null && wy !== null ? `   x ${wx.toFixed(1)}  y ${wy.toFixed(1)}` : ''}${s ? `   showing ${s.shown}/${s.live}` : ''}`,
        8,
        h - 8,
      )
    }
    raf = requestAnimationFrame(draw)
    return () => cancelAnimationFrame(raf)
  }, [])

  return (
    <div ref={box} className="relative min-h-[240px] h-full" style={{ background: MAP.void }}>
      <canvas
        ref={canvas}
        role="img"
        aria-label="Top-down map of the world. Click an entity to select it; drag to pan; scroll to zoom. The Entity tab lists the same entities."
        className="absolute inset-0 h-full w-full cursor-crosshair touch-none"
        data-theme={p.themeKey}
      />
    </div>
  )
}

interface Palette {
  fg: string
  fg3: string
  accent: string
  line: string
  bg: string
}

/** Text in the current fill colour with an ink rim, so labels read over grass, stone and dirt. */
function inked(ctx: CanvasRenderingContext2D, text: string, x: number, y: number) {
  const lw = ctx.lineWidth
  ctx.lineWidth = 3
  ctx.lineJoin = 'round'
  ctx.strokeStyle = MAP.ink
  ctx.strokeText(text, x, y)
  ctx.fillText(text, x, y)
  ctx.lineWidth = lw
}

/** Places as dashed circles, routes as numbered polylines with direction ticks. */
/** The World tab's hand edits over the map: added footprints outlined, removals crossed out, and a note while
 * the map shows a preview rather than the live world. */
function drawShape(
  ctx: CanvasRenderingContext2D,
  sh: ShapeOverlay,
  v: View,
  w: number,
  h: number,
  accent: string,
  rect: { x0: number; y0: number; x1: number; y1: number } | null,
) {
  const sx = (x: number) => (x - v.cx) * v.scale + w / 2
  const sy = (y: number) => (y - v.cy) * v.scale + h / 2
  ctx.lineWidth = 1.5
  ctx.strokeStyle = accent
  ctx.setLineDash([4, 3])
  for (const e of sh.edits) {
    if ('add' in e) {
      const [x0, y0, x1, y1] = e.add.b
      ctx.strokeRect(sx(Math.min(x0, x1)), sy(Math.min(y0, y1)), Math.abs(x1 - x0) * v.scale, Math.abs(y1 - y0) * v.scale)
    } else {
      const x = sx(e.remove[0])
      const y = sy(e.remove[1])
      ctx.beginPath()
      ctx.moveTo(x - 5, y - 5)
      ctx.lineTo(x + 5, y + 5)
      ctx.moveTo(x + 5, y - 5)
      ctx.lineTo(x - 5, y + 5)
      ctx.stroke()
    }
  }
  ctx.setLineDash([])
  if (rect && sh.tool === 'add') {
    ctx.fillStyle = accent
    ctx.globalAlpha = 0.25
    const x = sx(Math.min(rect.x0, rect.x1))
    const y = sy(Math.min(rect.y0, rect.y1))
    const rw = Math.abs(rect.x1 - rect.x0) * v.scale
    const rh = Math.abs(rect.y1 - rect.y0) * v.scale
    ctx.fillRect(x, y, rw, rh)
    ctx.globalAlpha = 1
    ctx.strokeRect(x, y, rw, rh)
  }
  if (sh.previewing) {
    const label = 'PREVIEW · not live until applied'
    ctx.font = "600 12px 'Instrument Sans Variable', system-ui, sans-serif"
    const tw = ctx.measureText(label).width
    ctx.fillStyle = MAP.ink
    ctx.fillRect(8, 8, tw + 16, 22)
    ctx.fillStyle = accent
    ctx.fillText(label, 16, 23)
  }
}

function drawEdit(
  ctx: CanvasRenderingContext2D,
  ed: EditOverlay,
  v: View,
  w: number,
  h: number,
  c: Palette,
  mouse: { x: number; y: number } | null,
  rect: { x0: number; y0: number; x1: number; y1: number } | null,
) {
  const sx = (x: number) => (x - v.cx) * v.scale + w / 2
  const sy = (y: number) => (y - v.cy) * v.scale + h / 2
  ctx.font = "12px 'Instrument Sans Variable', system-ui, sans-serif"
  ctx.lineWidth = 1

  ;(ed.walls ?? []).forEach((q, i) => {
    const on = ed.sel?.kind === 'wall' && ed.sel.index === i
    ctx.fillStyle = c.accent
    ctx.globalAlpha = on ? 0.55 : 0.28
    ctx.fillRect(sx(q[0]), sy(q[1]), (q[2] - q[0]) * v.scale, (q[3] - q[1]) * v.scale)
    ctx.globalAlpha = 1
    ctx.strokeStyle = c.accent
    ctx.lineWidth = on ? 2 : 1
    ctx.strokeRect(sx(q[0]), sy(q[1]), (q[2] - q[0]) * v.scale, (q[3] - q[1]) * v.scale)
    ctx.lineWidth = 1
  })
  if (rect) {
    ctx.strokeStyle = c.accent
    ctx.setLineDash([5, 4])
    ctx.strokeRect(sx(Math.min(rect.x0, rect.x1)), sy(Math.min(rect.y0, rect.y1)), Math.abs(rect.x1 - rect.x0) * v.scale, Math.abs(rect.y1 - rect.y0) * v.scale)
    ctx.setLineDash([])
  }

  ed.places.forEach((p, i) => {
    const on = ed.sel?.kind === 'place' && ed.sel.index === i
    const home = ed.home === p.name
    const x = sx(p.x)
    const y = sy(p.y)
    const r = Math.max(3, p.r * v.scale)
    ctx.strokeStyle = on || home ? c.accent : c.fg3
    ctx.fillStyle = on || home ? c.accent : c.fg3
    ctx.globalAlpha = on ? 0.18 : home ? 0.12 : 0.06
    ctx.beginPath()
    ctx.arc(x, y, r, 0, Math.PI * 2)
    ctx.fill()
    ctx.globalAlpha = 1
    ctx.setLineDash([4, 3])
    ctx.stroke()
    ctx.setLineDash([])
    ctx.beginPath()
    ctx.moveTo(x - 4, y)
    ctx.lineTo(x + 4, y)
    ctx.moveTo(x, y - 4)
    ctx.lineTo(x, y + 4)
    ctx.stroke()
    ctx.fillStyle = on || home ? c.accent : c.fg
    inked(ctx, home ? `${p.name} (home)` : p.name, x + 6, y - 6)
  })

  const drawLine = (pts: [number, number][], loop: boolean, color: string, dashed: boolean) => {
    if (pts.length === 0) return
    ctx.strokeStyle = color
    ctx.lineWidth = 1.5
    if (dashed) ctx.setLineDash([5, 4])
    ctx.beginPath()
    pts.forEach((pt, k) => (k === 0 ? ctx.moveTo(sx(pt[0]), sy(pt[1])) : ctx.lineTo(sx(pt[0]), sy(pt[1]))))
    ctx.stroke()
    if (loop && pts.length > 2) {
      ctx.setLineDash([2, 4])
      ctx.beginPath()
      const a = pts[pts.length - 1]!
      const b = pts[0]!
      ctx.moveTo(sx(a[0]), sy(a[1]))
      ctx.lineTo(sx(b[0]), sy(b[1]))
      ctx.stroke()
    }
    ctx.setLineDash([])
    ctx.lineWidth = 1
    // A tick at the middle of each segment shows which way it runs.
    ctx.fillStyle = color
    for (let k = 1; k < pts.length; k++) {
      const a = pts[k - 1]!
      const b = pts[k]!
      const mx = sx((a[0] + b[0]) / 2)
      const my = sy((a[1] + b[1]) / 2)
      const ang = Math.atan2(sy(b[1]) - sy(a[1]), sx(b[0]) - sx(a[0]))
      ctx.beginPath()
      ctx.moveTo(mx + Math.cos(ang) * 5, my + Math.sin(ang) * 5)
      ctx.lineTo(mx + Math.cos(ang + 2.5) * 5, my + Math.sin(ang + 2.5) * 5)
      ctx.lineTo(mx + Math.cos(ang - 2.5) * 5, my + Math.sin(ang - 2.5) * 5)
      ctx.fill()
    }
  }

  ed.routes.forEach((r, ri) => {
    const on = ed.sel?.kind === 'route' && ed.sel.index === ri
    const col = on ? c.accent : c.fg3
    drawLine(r.points, r.loop, col, false)
    r.points.forEach((pt, k) => {
      const x = sx(pt[0])
      const y = sy(pt[1])
      ctx.fillStyle = c.bg
      ctx.strokeStyle = col
      ctx.fillRect(x - 4, y - 4, 8, 8)
      ctx.strokeRect(x - 4, y - 4, 8, 8)
      if (k === 0) {
        ctx.fillStyle = on ? c.accent : c.fg
        inked(ctx, r.name, x + 7, y - 7)
      } else if (on || v.scale > 0.6) {
        ctx.fillStyle = on ? c.accent : c.fg3
        inked(ctx, String(k + 1), x + 7, y - 7)
      }
    })
  })

  if (ed.draft.length > 0) {
    const pts = mouse && ed.tool === 'route' ? [...ed.draft, [(mouse.x - w / 2) / v.scale + v.cx, (mouse.y - h / 2) / v.scale + v.cy] as [number, number]] : ed.draft
    drawLine(pts, false, c.accent, true)
    ed.draft.forEach((pt) => {
      ctx.fillStyle = c.accent
      ctx.fillRect(sx(pt[0]) - 3, sy(pt[1]) - 3, 6, 6)
    })
  }
}
