// The play view's colours live in CSS (the --hud-* variables in index.css), so the whole look can be changed
// in one place. The 3D scene reads them from there at startup and whenever the theme flips.

export interface Palette {
  bg: number
  line: number
  dim: number
  hot: number
  accent: number
}

const FALLBACK: Palette = { bg: 0x07060f, line: 0x00f0ff, dim: 0x1d3a5c, hot: 0xff2bd6, accent: 0xfcee0a }

function parse(s: string, fallback: number): number {
  const m = /^#([0-9a-f]{6})$/i.exec(s.trim())
  return m ? parseInt(m[1]!, 16) : fallback
}

export function readPalette(el: Element = document.documentElement): Palette {
  const cs = getComputedStyle(el)
  const get = (name: string, fb: number) => parse(cs.getPropertyValue(name), fb)
  return {
    bg: get('--hud-bg', FALLBACK.bg),
    line: get('--hud-line-3d', FALLBACK.line),
    dim: get('--hud-grid', FALLBACK.dim),
    hot: get('--hud-hot', FALLBACK.hot),
    accent: get('--hud-accent', FALLBACK.accent),
  }
}

export const css = (n: number) => '#' + n.toString(16).padStart(6, '0')
