import { useEffect, useRef, type MutableRefObject } from 'react'
import { KIND_DRIVEN, KIND_PLAYER, type State, type WorldGeom } from '../api'

interface Props {
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
    const local = (e: PointerEvent | WheelEvent) => {
      const r = cv.getBoundingClientRect()
      return { x: e.clientX - r.left, y: e.clientY - r.top }
    }
    const down = (e: PointerEvent) => {
      const l = local(e)
      drag = { x: l.x, y: l.y, cx: view.current.cx, cy: view.current.cy, moved: false }
      cv.setPointerCapture(e.pointerId)
    }
    const move = (e: PointerEvent) => {
      const l = local(e)
      mouse.current = l
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
      if (drag && !drag.moved) {
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
