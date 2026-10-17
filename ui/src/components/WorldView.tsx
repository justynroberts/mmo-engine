import { useEffect, useRef, type MutableRefObject } from 'react'
import { KIND_DRIVEN, KIND_PLAYER, type Place, type Route, type State, type WorldGeom } from '../api'
import type { Sel, Tool } from './Builder'

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

interface Props {
  edit?: EditOverlay | null
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
}

interface View {
  cx: number
  cy: number
  scale: number
  /** World size the view was last fitted to (0 = refit on next frame). */
  fittedFor: number
}

const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || '#888'

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
      if (ed && ed.tool === 'wall') {
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
        props.current.edit?.onAddWall(r.x0, r.y0, r.x1, r.y1)
        return
      }
      if (grab) {
        grab = null
        cv.releasePointerCapture(e.pointerId)
        return
      }
      const ed = props.current.edit
      if (drag && !drag.moved && ed) {
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
      const bg = css('--color-bg')
      const bg2 = css('--color-bg-2')
      const line = css('--color-line')
      const fg3 = css('--color-fg-3')
      const accent = css('--color-accent')
      const fg = css('--color-fg')
      ctx.fillStyle = bg2
      ctx.fillRect(0, 0, w, h)

      const sx = (x: number) => (x - v.cx) * v.scale + w / 2
      const sy = (y: number) => (y - v.cy) * v.scale + h / 2
      ctx.fillStyle = bg
      ctx.fillRect(sx(0), sy(0), worldSize * v.scale, worldSize * v.scale)

      // Grid every 64 units, only while it is readable.
      if (v.scale * 64 > 12) {
        ctx.strokeStyle = line
        ctx.globalAlpha = 0.6
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
      ctx.strokeStyle = fg3
      ctx.strokeRect(sx(0) + 0.5, sy(0) + 0.5, worldSize * v.scale, worldSize * v.scale)

      ctx.fillStyle = fg3
      ctx.globalAlpha = 0.35
      for (const b of pr.world?.boxes ?? []) {
        ctx.fillRect(sx(b[0]), sy(b[1]), (b[2] - b[0]) * v.scale, (b[3] - b[1]) * v.scale)
      }
      ctx.globalAlpha = 1

      if (pr.edit) drawEdit(ctx, pr.edit, v, w, h, { fg, fg3, accent, line, bg }, mouse.current, wallDrag.current)

      const s = pr.stateRef.current
      let sel: (number[] | undefined)
      if (s) {
        const px = Math.min(8, Math.max(3, v.scale * 1.4))
        const showBars = v.scale > 1.2
        for (const row of s.e) {
          const x = sx(row[1] ?? 0)
          const y = sy(row[2] ?? 0)
          if (x < -10 || y < -10 || x > w + 10 || y > h + 10) continue
          const kind = row[4] ?? 0
          const arch = row[3] ?? -1
          const col = arch >= 0 ? pr.colors[arch] ?? fg3 : fg3
          if (row[0] === pr.selected) sel = row
          if (kind === KIND_PLAYER) {
            // Players are plain bright squares; the accent is kept for selection and possession.
            ctx.fillStyle = fg
            ctx.fillRect(x - px / 2 - 0.5, y - px / 2 - 0.5, px + 1, px + 1)
          } else if (kind === KIND_DRIVEN) {
            ctx.fillStyle = col
            ctx.fillRect(x - px / 2, y - px / 2, px, px)
            ctx.strokeStyle = accent
            ctx.lineWidth = 1.5
            ctx.strokeRect(x - px / 2 - 2, y - px / 2 - 2, px + 4, px + 4)
          } else {
            ctx.fillStyle = col
            ctx.fillRect(x - px / 2, y - px / 2, px, px)
          }
          if (showBars && pr.healthIdx >= 0) {
            const hp = (row[5 + pr.healthIdx] ?? 0) / (pr.healthMax || 1)
            if (hp < 0.999) {
              ctx.fillStyle = line
              ctx.fillRect(x - px, y + px / 2 + 2, px * 2, 2)
              ctx.fillStyle = accent
              ctx.fillRect(x - px, y + px / 2 + 2, px * 2 * Math.max(0, Math.min(1, hp)), 2)
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

      // Readout.
      ctx.font = "11px 'Martian Mono', monospace"
      ctx.fillStyle = fg3
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
    <div ref={box} className="relative min-h-[240px] h-full bg-bg-2">
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

/** Places as dashed circles, routes as numbered polylines with direction ticks. */
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
  ctx.font = "11px 'Martian Mono', monospace"
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
    ctx.fillText(home ? `${p.name} (home)` : p.name, x + 6, y - 6)
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
        ctx.fillText(r.name, x + 7, y - 7)
      } else if (on || v.scale > 0.6) {
        ctx.fillStyle = on ? c.accent : c.fg3
        ctx.fillText(String(k + 1), x + 7, y - 7)
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
