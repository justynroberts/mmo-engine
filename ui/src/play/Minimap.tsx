import { useEffect, useRef, type MutableRefObject } from 'react'
import type { GameClient, PlayInfo } from './client'
import { css, readPalette } from './palette'
import type { Shared } from './shared'

const SIZE = 132
/** World units from the centre to the edge of the map. */
const RADIUS = 70

/** A north-up radar centred on you: walls, other characters, your target. */
export default function Minimap({ client, info, shared, dark }: { client: GameClient; info: PlayInfo; shared: MutableRefObject<Shared>; dark: boolean }) {
  const cv = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const el = cv.current
    const g = el?.getContext?.('2d')
    if (!el || !g) return // jsdom has no canvas; the map is a nicety
    const dpr = Math.min(window.devicePixelRatio || 1, 2)
    el.width = SIZE * dpr
    el.height = SIZE * dpr
    g.scale(dpr, dpr)
    const pal = readPalette(el)
    const k = SIZE / (2 * RADIUS)
    let raf = 0
    let lastDraw = 0
    // The arrow points the way we are going: worked out from where we were, and kept while we stand still.
    let heading = 0
    let was: { x: number; y: number } | null = null
    const draw = (now: number) => {
      raf = requestAnimationFrame(draw)
      if (now - lastDraw < 66) return
      lastDraw = now
      const { self, others } = client.last
      g.clearRect(0, 0, SIZE, SIZE)
      g.fillStyle = css(pal.bg)
      g.globalAlpha = 0.8
      g.fillRect(0, 0, SIZE, SIZE)
      g.globalAlpha = 1
      if (!self) return
      if (was && Math.hypot(self.x - was.x, self.y - was.y) > 0.05) heading = Math.atan2(self.x - was.x, -(self.y - was.y))
      was = { x: self.x, y: self.y }
      const sx = (x: number) => SIZE / 2 + (x - self.x) * k
      const sy = (y: number) => SIZE / 2 + (y - self.y) * k
      // World edge.
      g.strokeStyle = css(pal.hot)
      g.lineWidth = 1
      g.strokeRect(sx(0), sy(0), info.world_size * k, info.world_size * k)
      g.fillStyle = css(pal.dim)
      for (const [x0, y0, x1, y1] of client.walls.near(self.x, self.y, RADIUS)) {
        g.fillRect(sx(x0), sy(y0), (x1 - x0) * k, (y1 - y0) * k)
      }
      g.fillStyle = css(pal.line)
      const tid = shared.current.target?.id
      for (const e of others) {
        if (Math.abs(e.x - self.x) > RADIUS || Math.abs(e.y - self.y) > RADIUS) continue
        const x = sx(e.x)
        const y = sy(e.y)
        if (e.id === tid) {
          g.strokeStyle = css(pal.accent)
          g.strokeRect(x - 4, y - 4, 8, 8)
        }
        g.fillRect(x - 1.5, y - 1.5, 3, 3)
      }
      g.fillStyle = css(pal.hot)
      g.save()
      g.translate(SIZE / 2, SIZE / 2)
      g.rotate(heading) // 0 is north (up), clockwise like a compass
      g.beginPath()
      g.moveTo(0, -6)
      g.lineTo(4, 4)
      g.lineTo(0, 2)
      g.lineTo(-4, 4)
      g.closePath()
      g.fill()
      g.restore()
    }
    raf = requestAnimationFrame(draw)
    return () => cancelAnimationFrame(raf)
  }, [client, info, shared, dark])

  return <canvas ref={cv} role="img" aria-label="Minimap of your surroundings" className="block hud-panel" style={{ width: SIZE, height: SIZE }} />
}
