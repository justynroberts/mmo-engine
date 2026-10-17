import * as THREE from 'three'

// A small CPU-simulated point-sprite system. One draw call, soft round glow per particle, additive on dark
// backgrounds and ordinary alpha on light ones (additive light on a light page just vanishes).

const VERT = /* glsl */ `
attribute float aSize;
attribute vec4 aColor;
uniform float uScale;
varying vec4 vColor;
void main() {
  vColor = aColor;
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_PointSize = clamp(aSize * uScale / max(0.1, -mv.z), 0.0, 96.0);
  gl_Position = projectionMatrix * mv;
}
`
const FRAG = /* glsl */ `
varying vec4 vColor;
void main() {
  vec2 p = gl_PointCoord - 0.5;
  float d = length(p) * 2.0;
  if (d > 1.0) discard;
  float core = smoothstep(1.0, 0.0, d);
  float a = core * core * vColor.a;
  gl_FragColor = vec4(vColor.rgb, a);
}
`

export class Particles {
  readonly points: THREE.Points
  private readonly cap: number
  private readonly pos: Float32Array
  private readonly col: Float32Array
  private readonly size: Float32Array
  private readonly vel: Float32Array
  private readonly life: Float32Array
  private readonly max: Float32Array
  private readonly size0: Float32Array
  private readonly grav: Float32Array
  private readonly drag: Float32Array
  private readonly base: Float32Array
  private readonly mat: THREE.ShaderMaterial
  private next = 0
  private live = 0

  constructor(cap: number, additive: boolean) {
    this.cap = cap
    this.pos = new Float32Array(cap * 3)
    this.col = new Float32Array(cap * 4)
    this.size = new Float32Array(cap)
    this.vel = new Float32Array(cap * 3)
    this.life = new Float32Array(cap)
    this.max = new Float32Array(cap).fill(1)
    this.size0 = new Float32Array(cap)
    this.grav = new Float32Array(cap)
    this.drag = new Float32Array(cap)
    this.base = new Float32Array(cap * 3)
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.BufferAttribute(this.pos, 3).setUsage(THREE.DynamicDrawUsage))
    g.setAttribute('aColor', new THREE.BufferAttribute(this.col, 4).setUsage(THREE.DynamicDrawUsage))
    g.setAttribute('aSize', new THREE.BufferAttribute(this.size, 1).setUsage(THREE.DynamicDrawUsage))
    this.mat = new THREE.ShaderMaterial({
      vertexShader: VERT,
      fragmentShader: FRAG,
      transparent: true,
      depthWrite: false,
      blending: additive ? THREE.AdditiveBlending : THREE.NormalBlending,
      uniforms: { uScale: { value: 600 } },
    })
    this.points = new THREE.Points(g, this.mat)
    this.points.frustumCulled = false
    // Park everything out of sight until used.
    for (let i = 0; i < cap; i++) this.pos[i * 3 + 1] = -1e4
  }

  /** Pixels per world unit at distance 1, so sizes stay in world units whatever the window size. */
  setScale(heightPx: number, fovDeg: number) {
    this.mat.uniforms.uScale!.value = heightPx / (2 * Math.tan((fovDeg * Math.PI) / 360))
  }

  emit(x: number, y: number, z: number, vx: number, vy: number, vz: number, life: number, size: number, c: THREE.Color, alpha = 1, gravity = 0, drag = 0) {
    const i = this.next
    this.next = (this.next + 1) % this.cap
    const p = i * 3
    this.pos[p] = x
    this.pos[p + 1] = y
    this.pos[p + 2] = z
    this.vel[p] = vx
    this.vel[p + 1] = vy
    this.vel[p + 2] = vz
    this.life[i] = life
    this.max[i] = life
    this.size0[i] = size
    this.grav[i] = gravity
    this.drag[i] = drag
    const q = i * 4
    this.col[q] = c.r
    this.col[q + 1] = c.g
    this.col[q + 2] = c.b
    this.col[q + 3] = alpha
    this.live = Math.min(this.cap, this.live + 1)
  }

  /** A sphere of sparks. */
  burst(x: number, y: number, z: number, c: THREE.Color, n: number, speed: number, life = 0.7, size = 0.5) {
    for (let k = 0; k < n; k++) {
      const a = Math.random() * Math.PI * 2
      const u = Math.random() * 2 - 1
      const r = Math.sqrt(1 - u * u)
      const s = speed * (0.35 + Math.random() * 0.65)
      this.emit(x, y, z, Math.cos(a) * r * s, (u * 0.6 + 0.5) * s, Math.sin(a) * r * s, life * (0.6 + Math.random() * 0.6), size * (0.5 + Math.random()), c, 1, -14, 1.6)
    }
  }

  update(dt: number) {
    let any = 0
    for (let i = 0; i < this.cap; i++) {
      const l = this.life[i]!
      if (l <= 0) continue
      const nl = l - dt
      const p = i * 3
      if (nl <= 0) {
        this.life[i] = 0
        this.pos[p + 1] = -1e4
        this.size[i] = 0
        continue
      }
      any++
      this.life[i] = nl
      const k = Math.max(0, 1 - this.drag[i]! * dt)
      this.vel[p] = this.vel[p]! * k
      this.vel[p + 1] = this.vel[p + 1]! * k + this.grav[i]! * dt
      this.vel[p + 2] = this.vel[p + 2]! * k
      this.pos[p] = this.pos[p]! + this.vel[p]! * dt
      this.pos[p + 1] = this.pos[p + 1]! + this.vel[p + 1]! * dt
      this.pos[p + 2] = this.pos[p + 2]! + this.vel[p + 2]! * dt
      if (this.pos[p + 1]! < 0.05) {
        this.pos[p + 1] = 0.05
        this.vel[p + 1] = Math.abs(this.vel[p + 1]!) * 0.35
      }
      const t = nl / this.max[i]!
      this.size[i] = this.size0[i]! * (0.35 + 0.65 * t)
      this.col[i * 4 + 3] = Math.min(1, t * 1.6)
    }
    this.live = any
    const g = this.points.geometry
    ;(g.getAttribute('position') as THREE.BufferAttribute).needsUpdate = true
    ;(g.getAttribute('aColor') as THREE.BufferAttribute).needsUpdate = true
    ;(g.getAttribute('aSize') as THREE.BufferAttribute).needsUpdate = true
  }

  /** Ambient motes: fill the pool with slow drifters in a box that follows `cx, cz`. Call `drift` each frame. */
  seedMotes(cx: number, cz: number, span: number, height: number, c: THREE.Color, size: number) {
    for (let i = 0; i < this.cap; i++) {
      const p = i * 3
      this.pos[p] = cx + (Math.random() - 0.5) * span
      this.pos[p + 1] = Math.random() * height
      this.pos[p + 2] = cz + (Math.random() - 0.5) * span
      this.base[p] = Math.random() * Math.PI * 2
      this.base[p + 1] = 0.3 + Math.random() * 0.9
      this.base[p + 2] = Math.random()
      this.life[i] = 1e9
      this.max[i] = 1e9
      const q = i * 4
      this.col[q] = c.r
      this.col[q + 1] = c.g
      this.col[q + 2] = c.b
      this.size[i] = size * (0.4 + Math.random() * 0.9)
    }
  }

  drift(t: number, cx: number, cz: number, span: number, height: number, alpha: number) {
    const h = span / 2
    for (let i = 0; i < this.cap; i++) {
      const p = i * 3
      const ph = this.base[p]!
      const sp = this.base[p + 1]!
      let x = this.pos[p]! + Math.cos(t * 0.3 * sp + ph) * 0.02
      let y = this.pos[p + 1]! + (0.012 + sp * 0.012)
      let z = this.pos[p + 2]! + Math.sin(t * 0.27 * sp + ph) * 0.02
      if (x - cx > h) x -= span
      else if (x - cx < -h) x += span
      if (z - cz > h) z -= span
      else if (z - cz < -h) z += span
      if (y > height) y = 0
      this.pos[p] = x
      this.pos[p + 1] = y
      this.pos[p + 2] = z
      // Twinkle, and fade at the very top and bottom of the box.
      const edge = Math.min(1, y / 2, (height - y) / 4)
      this.col[i * 4 + 3] = alpha * edge * (0.55 + 0.45 * Math.sin(t * 1.3 * sp + ph * 3))
    }
    const g = this.points.geometry
    ;(g.getAttribute('position') as THREE.BufferAttribute).needsUpdate = true
    ;(g.getAttribute('aColor') as THREE.BufferAttribute).needsUpdate = true
    ;(g.getAttribute('aSize') as THREE.BufferAttribute).needsUpdate = true
  }

  dispose() {
    this.points.geometry.dispose()
    this.mat.dispose()
  }
}

/** A soft radial gradient, white in the middle fading to nothing, for ground glows. */
export function glowTexture(): THREE.CanvasTexture {
  const c = document.createElement('canvas')
  c.width = c.height = 128
  const g = c.getContext('2d')
  if (g) {
    const r = g.createRadialGradient(64, 64, 0, 64, 64, 64)
    r.addColorStop(0, 'rgba(255,255,255,1)')
    r.addColorStop(0.35, 'rgba(255,255,255,0.35)')
    r.addColorStop(1, 'rgba(255,255,255,0)')
    g.fillStyle = r
    g.fillRect(0, 0, 128, 128)
  }
  return new THREE.CanvasTexture(c)
}

/** A tile of windows: mostly dark, a few lit, tinted by `lit` colours. */
export function windowTexture(bg: string, lit: string[]): THREE.CanvasTexture {
  const c = document.createElement('canvas')
  c.width = c.height = 128
  const g = c.getContext('2d')
  if (g) {
    g.fillStyle = bg
    g.fillRect(0, 0, 128, 128)
    for (let y = 0; y < 8; y++) {
      for (let x = 0; x < 8; x++) {
        const on = Math.random() < 0.28
        g.fillStyle = on ? lit[Math.floor(Math.random() * lit.length)]! : 'rgba(255,255,255,0.05)'
        g.globalAlpha = on ? 0.55 + Math.random() * 0.45 : 1
        g.fillRect(x * 16 + 4, y * 16 + 5, 8, 6)
      }
    }
    g.globalAlpha = 1
  }
  const t = new THREE.CanvasTexture(c)
  t.wrapS = t.wrapT = THREE.RepeatWrapping
  t.colorSpace = THREE.SRGBColorSpace
  return t
}
