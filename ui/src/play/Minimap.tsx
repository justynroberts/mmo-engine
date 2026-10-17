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
      const sx = (x: number) => SIZE / 2 + (x - self.x) * k
      const sy = (y: number) => SIZE / 2 + (y - self.y) * k
      // World edge.
      g.strokeStyle = css(pal.hot)
      g.lineWidth = 1
      g.strokeRect(sx(0), sy(0), info.world_size * k, info.world_size * k)
      g.fillStyle = css(pal.dim)
      for (const [x0, y0, x1, y1] of info.boxes) {
        if (x1 < self.x - RADIUS || x0 > self.x + RADIUS || y1 < self.y - RADIUS || y0 > self.y + RADIUS) continue
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
      g.beginPath()
      g.moveTo(SIZE / 2, SIZE / 2 - 5)
      g.lineTo(SIZE / 2 + 4, SIZE / 2 + 4)
      g.lineTo(SIZE / 2 - 4, SIZE / 2 + 4)
      g.closePath()
      g.fill()
    }
    raf = requestAnimationFrame(draw)
    return () => cancelAnimationFrame(raf)
  }, [client, info, shared, dark])

  return <canvas ref={cv} role="img" aria-label="Minimap of your surroundings" className="block hud-panel" style={{ width: SIZE, height: SIZE }} />
}
