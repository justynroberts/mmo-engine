import { useEffect, useRef, type MutableRefObject } from 'react'
import * as THREE from 'three'
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js'
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js'
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js'
import { OutputPass } from 'three/examples/jsm/postprocessing/OutputPass.js'
import { glowTexture, Particles, windowTexture } from './fx'
import type { GameClient, PlayInfo } from './client'
import { css, readPalette, type Palette } from './palette'
import { clampZoom, healthAttr, type Shared, type Target } from './shared'

interface Props {
  client: GameClient
  info: PlayInfo
  shared: MutableRefObject<Shared>
  /** Only here so the scene is rebuilt (and the palette re-read) when the theme flips. */
  dark: boolean
}

const CAP = 2048
const PICK_RADIUS = 5
const MAX_FLOATERS = 24
const FLOAT_LIFE = 0.9
const MAX_WAVES = 8
const WAVE_LIFE = 0.7

interface Wave {
  mesh: THREE.Mesh
  age: number
  max: number
  size: number
}

/** A square grid of thin strips on the ground; every fourth line is brighter. */
function thinGrid(size: number, step: number, w: number, minor: THREE.Color, major: THREE.Color): THREE.Mesh {
  const n = Math.round(size / step)
  const half = size / 2
  const pos: number[] = []
  const col: number[] = []
  const strip = (x0: number, z0: number, x1: number, z1: number, c: THREE.Color) => {
    pos.push(x0, 0, z0, x1, 0, z0, x1, 0, z1, x0, 0, z0, x1, 0, z1, x0, 0, z1)
    for (let i = 0; i < 6; i++) col.push(c.r, c.g, c.b)
  }
  for (let i = 0; i <= n; i++) {
    const k = -half + i * step
    const c = i % 4 === 0 ? major : minor
    strip(-half, k - w / 2, half, k + w / 2, c)
    strip(k - w / 2, -half, k + w / 2, half, c)
  }
  const g = new THREE.BufferGeometry()
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3))
  g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3))
  return new THREE.Mesh(g, new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.DoubleSide }))
}

interface Floater {
  sprite: THREE.Sprite
  age: number
  y: number
}

/** The world around you, as neon wireframe, from just above and behind. */
export default function PlayScene({ client, info, shared, dark }: Props) {
  const box = useRef<HTMLDivElement>(null)
  const ref = useRef({ client, info, shared })
  ref.current = { client, info, shared }

  useEffect(() => {
    const host = box.current
    if (!host) return
    let renderer: THREE.WebGLRenderer
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true })
    } catch {
      host.textContent = 'WebGL is not available in this browser.'
      return
    }
    const pal: Palette = readPalette(host)
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2))
    renderer.setClearColor(pal.bg, 1)
    const cv = renderer.domElement
    cv.className = 'absolute inset-0 h-full w-full touch-none'
    cv.setAttribute('role', 'img')
    cv.setAttribute('aria-label', 'Wireframe view of the world around your character. Move with W A S D or the arrow keys, act with space, click something to target it.')
    host.appendChild(cv)

    const half = info.world_size / 2
    const scene = new THREE.Scene()
    scene.fog = new THREE.Fog(pal.bg, 50, 150)
    const camera = new THREE.PerspectiveCamera(55, 1, 0.5, 900)

    // Post: bloom makes everything bright glow. Off on the light theme, where glow on a pale page only muddies it.
    const target = new THREE.WebGLRenderTarget(64, 64, { type: THREE.HalfFloatType, samples: 4 })
    const composer = new EffectComposer(renderer, target)
    composer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5))
    composer.addPass(new RenderPass(scene, camera))
    const bloom = new UnrealBloomPass(new THREE.Vector2(64, 64), 0.8, 0.55, 0.22)
    bloom.enabled = dark
    composer.addPass(bloom)
    composer.addPass(new OutputPass())
    const glow = dark ? 1.8 : 1

    // The grid follows the player, snapped to its spacing so it never appears to slide. Drawn as thin quads, not GL
    // lines: line rasterisation is the least consistent part of WebGL (software renderers drop steep lines).
    const STEP = 8
    const minor = new THREE.Color(pal.dim)
    const major = new THREE.Color(pal.dim).lerp(new THREE.Color(pal.hot), 0.45)
    const grid = thinGrid(240, STEP, 0.07, minor, major)
    scene.add(grid)

    const geoms: THREE.BufferGeometry[] = []
    const disposables: { dispose(): void }[] = []
    const wallMat = new THREE.LineBasicMaterial({ color: new THREE.Color(pal.line).multiplyScalar(glow) })
    disposables.push(wallMat)
    const edge = new THREE.EdgesGeometry(new THREE.BoxGeometry(info.world_size, 4, info.world_size))
    edge.translate(0, 2, 0)
    scene.add(new THREE.LineSegments(edge, new THREE.LineBasicMaterial({ color: new THREE.Color(pal.hot).multiplyScalar(glow) })))
    geoms.push(edge)

    // Buildings: dark slabs with lit windows and a glowing outline, each a different height.
    const winBg = dark ? '#0b0a1a' : '#cdc5e0'
    const winTex = windowTexture(winBg, [css(pal.line), css(pal.hot), css(pal.accent)])
    disposables.push(winTex)
    const plain = new THREE.MeshBasicMaterial({ color: dark ? 0x0d0b1e : 0xc3badb })
    disposables.push(plain)
    info.boxes.forEach(([x0, y0, x1, y1], i) => {
      const w = x1 - x0
      const d = y1 - y0
      const h = 7 + ((i * 7919) % 9)
      const face = (across: number) => {
        const t = winTex.clone()
        t.needsUpdate = true
        t.repeat.set(Math.max(1, Math.round(across / 5)), Math.max(1, Math.round(h / 4)))
        const m = new THREE.MeshBasicMaterial({ map: t, color: dark ? 0xffffff : 0xdcd4ee })
        disposables.push(t, m)
        return m
      }
      const sideX = face(d)
      const sideZ = face(w)
      const g = new THREE.BoxGeometry(w, h, d)
      const mesh = new THREE.Mesh(g, [sideX, sideX, plain, plain, sideZ, sideZ])
      mesh.position.set((x0 + x1) / 2 - half, h / 2, (y0 + y1) / 2 - half)
      scene.add(mesh)
      const eg = new THREE.EdgesGeometry(g)
      eg.translate(mesh.position.x, mesh.position.y, mesh.position.z)
      geoms.push(g, eg)
      scene.add(new THREE.LineSegments(eg, wallMat))
    })

    // Everyone else: instanced octahedra, tinted toward the warning colour as they get hurt.
    const others = new THREE.InstancedMesh(new THREE.OctahedronGeometry(0.9), new THREE.MeshBasicMaterial({ color: 0xffffff, wireframe: true }), CAP)
    const cores = new THREE.InstancedMesh(new THREE.OctahedronGeometry(0.5), new THREE.MeshBasicMaterial({ color: 0xffffff }), CAP)
    cores.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(CAP * 3), 3)
    cores.frustumCulled = false
    cores.count = 0
    const gtex = glowTexture()
    disposables.push(gtex)
    const pools = new THREE.InstancedMesh(
      new THREE.PlaneGeometry(1, 1).rotateX(-Math.PI / 2),
      new THREE.MeshBasicMaterial({ map: gtex, transparent: true, depthWrite: false, blending: dark ? THREE.AdditiveBlending : THREE.NormalBlending, opacity: dark ? 0.55 : 0.4 }),
      CAP,
    )
    pools.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(CAP * 3), 3)
    pools.frustumCulled = false
    pools.count = 0
    scene.add(cores, pools)

    // Things lying on the ground: spinning lit gems with a pool of light, in the item's colour.
    const GCAP = 128
    const gems = new THREE.InstancedMesh(new THREE.OctahedronGeometry(0.55).scale(1, 1.5, 1), new THREE.MeshBasicMaterial({ color: 0xffffff }), GCAP)
    gems.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(GCAP * 3), 3)
    gems.frustumCulled = false
    gems.count = 0
    const gemPools = new THREE.InstancedMesh(
      new THREE.PlaneGeometry(1, 1).rotateX(-Math.PI / 2),
      new THREE.MeshBasicMaterial({ map: gtex, transparent: true, depthWrite: false, blending: dark ? THREE.AdditiveBlending : THREE.NormalBlending, opacity: dark ? 0.7 : 0.5 }),
      GCAP,
    )
    gemPools.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(GCAP * 3), 3)
    gemPools.frustumCulled = false
    gemPools.count = 0
    scene.add(gems, gemPools)
    const gemSeen = new Map<number, { x: number; z: number; c: THREE.Color }>()
    others.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(CAP * 3), 3)
    others.frustumCulled = false
    others.count = 0
    const me = new THREE.Mesh(new THREE.ConeGeometry(0.9, 2.6, 4), new THREE.MeshBasicMaterial({ color: new THREE.Color(pal.hot).multiplyScalar(glow), wireframe: true }))
    const meCore = new THREE.Mesh(new THREE.ConeGeometry(0.5, 1.6, 4), new THREE.MeshBasicMaterial({ color: new THREE.Color(pal.hot).multiplyScalar(glow) }))
    const mePool = new THREE.Mesh(
      new THREE.PlaneGeometry(1, 1).rotateX(-Math.PI / 2),
      new THREE.MeshBasicMaterial({ map: gtex, color: pal.hot, transparent: true, depthWrite: false, blending: dark ? THREE.AdditiveBlending : THREE.NormalBlending, opacity: dark ? 0.5 : 0.35 }),
    )
    mePool.scale.set(14, 1, 14)
    const lightPool = new THREE.Mesh(
      new THREE.PlaneGeometry(1, 1).rotateX(-Math.PI / 2),
      new THREE.MeshBasicMaterial({ map: gtex, color: pal.line, transparent: true, depthWrite: false, blending: dark ? THREE.AdditiveBlending : THREE.NormalBlending, opacity: dark ? 0.16 : 0.1 }),
    )
    lightPool.scale.set(90, 1, 90)
    scene.add(meCore, mePool, lightPool)
    for (const o of [me, meCore, mePool, lightPool]) disposables.push(o.geometry, o.material as THREE.Material)
    const ringMat = new THREE.MeshBasicMaterial({ color: pal.accent, side: THREE.DoubleSide })
    const ring = new THREE.Mesh(new THREE.RingGeometry(1.4, 1.6, 32).rotateX(-Math.PI / 2), ringMat)
    ring.visible = false
    scene.add(others, me, ring)

    // Target beam, shockwaves, particles.
    const beam = new THREE.Mesh(
      new THREE.CylinderGeometry(0.07, 0.07, 18, 6, 1, true),
      new THREE.MeshBasicMaterial({ color: new THREE.Color(pal.accent).multiplyScalar(glow), transparent: true, opacity: 0.5, depthWrite: false, blending: dark ? THREE.AdditiveBlending : THREE.NormalBlending }),
    )
    beam.visible = false
    scene.add(beam)
    disposables.push(beam.geometry, beam.material as THREE.Material)
    const waveGeo = new THREE.RingGeometry(0.86, 1, 48).rotateX(-Math.PI / 2)
    disposables.push(waveGeo)
    const waves: Wave[] = []
    const wave = (x: number, z: number, color: THREE.Color, size: number) => {
      const mat = new THREE.MeshBasicMaterial({ color, transparent: true, depthWrite: false, side: THREE.DoubleSide, blending: dark ? THREE.AdditiveBlending : THREE.NormalBlending })
      const mesh = new THREE.Mesh(waveGeo, mat)
      mesh.position.set(x, 0.15, z)
      scene.add(mesh)
      waves.push({ mesh, age: 0, max: WAVE_LIFE, size })
      if (waves.length > MAX_WAVES) {
        const o = waves.shift()!
        scene.remove(o.mesh)
        ;(o.mesh.material as THREE.Material).dispose()
      }
    }
    const sparks = new Particles(3000, dark)
    const motes = new Particles(520, dark)
    const MOTE_SPAN = 100
    const MOTE_H = 30
    motes.seedMotes(0, 0, MOTE_SPAN, MOTE_H, new THREE.Color(dark ? pal.line : pal.hot), dark ? 0.35 : 0.3)
    scene.add(motes.points, sparks.points)
    const cHot = new THREE.Color(pal.hot).multiplyScalar(glow)
    const cAcc = new THREE.Color(pal.accent).multiplyScalar(glow)
    const cTmp = new THREE.Color()
    let shake = 0
    const shakeV = new THREE.Vector3()
    const prevPos = new Map<number, { x: number; z: number; hp: number | null; c: THREE.Color }>()
    let selfPrev: { x: number; z: number } | null = null

    const bars = new THREE.LineSegments(
      new THREE.BufferGeometry().setAttribute('position', new THREE.BufferAttribute(new Float32Array(CAP * 12), 3)),
      new THREE.LineBasicMaterial({ color: pal.accent }),
    )
    bars.frustumCulled = false
    scene.add(bars)

    // Weather: the fog closes in and streaks fall around you, in the weather's colour.
    const RAIN = 700
    const rainGeo = new THREE.BufferGeometry().setAttribute('position', new THREE.BufferAttribute(new Float32Array(RAIN * 6), 3))
    const rainMat = new THREE.LineBasicMaterial({ color: pal.line, transparent: true, opacity: 0.55 })
    const rain = new THREE.LineSegments(rainGeo, rainMat)
    rain.frustumCulled = false
    rain.visible = false
    scene.add(rain)
    const rainPos = Array.from({ length: RAIN }, () => ({ x: Math.random() * 80 - 40, y: Math.random() * 40, z: Math.random() * 80 - 40 }))
    const fogBase = new THREE.Color(pal.bg)
    const fogTint = new THREE.Color()
    const fog = scene.fog as THREE.Fog

    // Floating damage numbers.
    const textures = new Map<string, THREE.CanvasTexture>()
    const labelTexture = (text: string, color: number) => {
      const key = `${text}/${color}`
      let t = textures.get(key)
      if (!t) {
        const c = document.createElement('canvas')
        c.width = 128
        c.height = 48
        const g = c.getContext('2d')
        if (g) {
          g.font = '700 28px "Martian Mono", ui-monospace, monospace'
          g.textAlign = 'center'
          g.textBaseline = 'middle'
          g.lineWidth = 5
          g.strokeStyle = css(pal.bg)
          g.strokeText(text, 64, 25)
          g.fillStyle = css(color)
          g.fillText(text, 64, 25)
        }
        t = new THREE.CanvasTexture(c)
        textures.set(key, t)
      }
      return t
    }
    const floaters: Floater[] = []
    const floatAt = (x: number, z: number, text: string, color: number) => {
      const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: labelTexture(text, color), transparent: true, depthTest: false }))
      sprite.scale.set(6, 2.25, 1)
      sprite.position.set(x, 3.2, z)
      scene.add(sprite)
      floaters.push({ sprite, age: 0, y: 3.2 })
      if (floaters.length > MAX_FLOATERS) drop(floaters.shift()!)
    }
    const drop = (f: Floater) => {
      scene.remove(f.sprite)
      f.sprite.material.dispose()
    }

    const fit = () => {
      const w = Math.max(50, host.clientWidth)
      const h = Math.max(50, host.clientHeight)
      renderer.setSize(w, h, false)
      composer.setSize(w, h)
      bloom.setSize(w, h)
      camera.aspect = w / h
      camera.updateProjectionMatrix()
      const pr = Math.min(window.devicePixelRatio || 1, 1.5)
      sparks.setScale(h * pr, camera.fov)
      motes.setScale(h * pr, camera.fov)
    }
    fit()
    const ro = new ResizeObserver(fit)
    ro.observe(host)

    // Picking: a click or tap on the ground picks the nearest character within a few units of the point.
    const ray = new THREE.Raycaster()
    const plane = new THREE.Plane(new THREE.Vector3(0, 1, 0), -1)
    const hit = new THREE.Vector3()
    const ndc = new THREE.Vector2()
    let world: { id: number; x: number; z: number }[] = []
    const onDown = (e: PointerEvent) => {
      if (e.pointerType === 'mouse' && e.button !== 0) return
      const r = cv.getBoundingClientRect()
      ndc.set(((e.clientX - r.left) / r.width) * 2 - 1, -(((e.clientY - r.top) / r.height) * 2 - 1))
      ray.setFromCamera(ndc, camera)
      if (!ray.ray.intersectPlane(plane, hit)) return
      let best: number | null = null
      let bd = PICK_RADIUS * ref.current.shared.current.zoom
      for (const w of world) {
        const d = Math.hypot(w.x - hit.x, w.z - hit.z)
        if (d < bd) {
          bd = d
          best = w.id
        }
      }
      ref.current.shared.current.selected = best
    }
    const onWheel = (e: WheelEvent) => {
      const s = ref.current.shared.current
      s.zoom = clampZoom(s.zoom * (e.deltaY > 0 ? 1.1 : 1 / 1.1))
      e.preventDefault()
    }
    cv.addEventListener('pointerdown', onDown)
    cv.addEventListener('wheel', onWheel, { passive: false })

    const tmp = new THREE.Object3D()
    const tint = new THREE.Color()
    const cLine = new THREE.Color(pal.line)
    const cWarn = new THREE.Color(pal.accent)
    const cam = new THREE.Vector3()
    const camLook = new THREE.Vector3()
    const hpPrev = new Map<number, number>()
    let camReady = false
    let last = performance.now()
    let fpsAt = last
    let slow = 0
    let post = dark
    let frames = 0
    let raf = 0
    const rangeOf = () => ref.current.info.actions[0]?.range ?? 8
    const frame = (now: number) => {
      raf = requestAnimationFrame(frame)
      const dt = Math.min(0.1, (now - last) / 1000)
      last = now
      frames++
      const { client: c, info: inf, shared: sh } = ref.current
      const s = sh.current
      if (now - fpsAt >= 1000) {
        s.fps = Math.round((frames * 1000) / (now - fpsAt))
        frames = 0
        fpsAt = now
      }
      // A slow device sheds the expensive parts (bloom, supersampled target) rather than stuttering.
      slow = dt > 0.07 ? slow + 1 : Math.max(0, slow - 1)
      if (slow >= 6 && post) {
        post = false
        target.dispose()
      }
      const fr = c.frame(now)
      const hpa = healthAttr(inf)
      const hpOf = (attrs: Record<string, number>) => (hpa && attrs[hpa.name] !== undefined ? attrs[hpa.name]! : undefined)
      const frac = (v: number | undefined) => (v !== undefined && hpa ? Math.max(0, Math.min(1, (v - hpa.min) / (hpa.max - hpa.min))) : null)

      // Damage numbers: any drop in health since the last frame.
      const seen = new Set<number>()
      for (const e of fr.self ? [fr.self, ...fr.others] : fr.others) {
        seen.add(e.id)
        const v = hpOf(e.attrs)
        if (v === undefined) continue
        const p = hpPrev.get(e.id)
        if (p !== undefined && v < p - 0.5) {
          floatAt(e.x - half, e.y - half, `-${Math.round(p - v)}`, e.id === c.entity ? pal.hot : pal.accent)
          const mine = e.id === c.entity
          sparks.burst(e.x - half, 1.4, e.y - half, mine ? cHot : cAcc, mine ? 26 : 14, mine ? 14 : 10, 0.6, 0.55)
          wave(e.x - half, e.y - half, (mine ? cHot : cAcc).clone(), mine ? 5 : 3.2)
          if (mine) shake = Math.min(1.2, shake + 0.55)
        }
        hpPrev.set(e.id, v)
      }
      for (const id of hpPrev.keys()) if (!seen.has(id)) hpPrev.delete(id)
      // Something that was nearly dead and is gone: a death, not just walking out of range.
      for (const [id, pv] of prevPos) {
        if (seen.has(id)) continue
        if (pv.hp !== null && pv.hp < 0.35) {
          sparks.burst(pv.x, 1.2, pv.z, cTmp.copy(pv.c).multiplyScalar(1.4), 60, 18, 1.0, 0.7)
          sparks.burst(pv.x, 1.2, pv.z, cAcc, 20, 9, 0.8, 0.5)
          wave(pv.x, pv.z, pv.c.clone().multiplyScalar(1.4), 11)
        }
        prevPos.delete(id)
      }

      // Things on the ground.
      {
        const items = s.items
        const seenG = new Set<number>()
        let gn = 0
        for (const g of c.ground) {
          const it = items[g.item]
          if (!it || gn >= GCAP) continue
          const x = g.x - half
          const z = g.y - half
          seenG.add(g.id)
          tint.set(it.color).multiplyScalar(glow)
          if (!gemSeen.has(g.id)) gemSeen.set(g.id, { x, z, c: new THREE.Color(it.color) })
          tmp.position.set(x, 1.1 + Math.sin(now / 380 + g.id) * 0.25, z)
          tmp.rotation.set(0, now / 700 + g.id, 0)
          tmp.scale.set(1, 1, 1)
          tmp.updateMatrix()
          gems.setMatrixAt(gn, tmp.matrix)
          gems.setColorAt(gn, tint)
          tmp.position.set(x, 0.1, z)
          tmp.rotation.set(0, 0, 0)
          tmp.scale.set(5.5, 1, 5.5)
          tmp.updateMatrix()
          gemPools.setMatrixAt(gn, tmp.matrix)
          gemPools.setColorAt(gn, tint)
          tmp.scale.set(1, 1, 1)
          if (Math.random() < 0.18) sparks.emit(x + (Math.random() - 0.5) * 1.4, 0.4, z + (Math.random() - 0.5) * 1.4, 0, 1.6 + Math.random(), 0, 0.9, 0.3, tint, 0.8, 0, 0)
          gn++
        }
        gems.count = gn
        gemPools.count = gn
        for (const m of [gems, gemPools]) {
          m.instanceMatrix.needsUpdate = true
          if (m.instanceColor) m.instanceColor.needsUpdate = true
        }
        // One that vanished near you was picked up (or fell to someone nearby): a little burst where it lay.
        for (const [id, gv] of gemSeen) {
          if (seenG.has(id)) continue
          gemSeen.delete(id)
          if (fr.self && Math.hypot(gv.x - (fr.self.x - half), gv.z - (fr.self.y - half)) < 8) {
            sparks.burst(gv.x, 1.1, gv.z, cTmp.copy(gv.c).multiplyScalar(glow), 22, 8, 0.7, 0.45)
            wave(gv.x, gv.z, gv.c.clone().multiplyScalar(glow), 3)
          }
        }
      }

      // Others.
      world = []
      let n = 0
      let nb = 0
      const bp = bars.geometry.getAttribute('position') as THREE.BufferAttribute
      const self = fr.self
      let nearest: { id: number; d: number } | null = null
      const chosen = s.selected
      for (const e of fr.others) {
        const x = e.x - half
        const z = e.y - half
        world.push({ id: e.id, x, z })
        const hp = frac(hpOf(e.attrs))
        if (n < CAP) {
          tmp.position.set(x, 1, z)
          tmp.rotation.set(0, now / 1500 + e.id, 0)
          tmp.updateMatrix()
          others.setMatrixAt(n, tmp.matrix)
          tint.copy(cLine).lerp(cWarn, hp === null ? 0 : (1 - hp) * 0.9)
          tint.multiplyScalar(glow)
          others.setColorAt(n, tint)
          prevPos.set(e.id, { x, z, hp, c: prevPos.get(e.id)?.c ?? new THREE.Color() })
          prevPos.get(e.id)!.c.copy(tint).multiplyScalar(1 / glow)
          const pulse = 0.8 + Math.sin(now / 260 + e.id) * 0.25
          tmp.scale.set(pulse, pulse, pulse)
          tmp.updateMatrix()
          cores.setMatrixAt(n, tmp.matrix)
          cores.setColorAt(n, tint)
          tmp.scale.set(1, 1, 1)
          tmp.position.set(x, 0.08, z)
          tmp.rotation.set(0, 0, 0)
          tmp.scale.set(6.5, 1, 6.5)
          tmp.updateMatrix()
          pools.setMatrixAt(n, tmp.matrix)
          pools.setColorAt(n, tint)
          tmp.scale.set(1, 1, 1)
          if (n < 60 && Math.random() < 0.35) sparks.emit(x + (Math.random() - 0.5), 0.6, z + (Math.random() - 0.5), 0, 0.8 + Math.random(), 0, 0.7, 0.35, tint, 0.55, 0, 0.5)
          n++
        }
        if (hp !== null && hp < 0.995 && nb < CAP) {
          const w = 1.6
          const y = 2.6
          const hx = x - w / 2 + w * hp
          bp.setXYZ(nb * 4, x - w / 2, y, z)
          bp.setXYZ(nb * 4 + 1, hx, y, z)
          bp.setXYZ(nb * 4 + 2, x - w / 2, y + 0.01, z)
          bp.setXYZ(nb * 4 + 3, hx, y + 0.01, z)
          nb++
        }
        if (self) {
          const d = Math.hypot(e.x - self.x, e.y - self.y)
          if (!nearest || d < nearest.d) nearest = { id: e.id, d }
        }
      }
      others.count = n
      cores.count = n
      pools.count = n
      for (const m of [others, cores, pools]) {
        m.instanceMatrix.needsUpdate = true
        if (m.instanceColor) m.instanceColor.needsUpdate = true
      }
      bars.geometry.setDrawRange(0, nb * 4)
      bp.needsUpdate = true

      // A chosen target that went out of sight is forgotten; otherwise the nearest is the default.
      const locked = chosen !== null && fr.others.some((o) => o.id === chosen)
      if (!locked) s.selected = null
      const tid = locked ? chosen : (nearest?.id ?? null)
      const to = tid === null ? undefined : fr.others.find((o) => o.id === tid)
      let tgt: Target | null = null
      let tx = 0
      let tz = 0
      if (self && to) {
        tx = to.x - half
        tz = to.y - half
        const v = hpOf(to.attrs)
        const dist = Math.hypot(to.x - self.x, to.y - self.y)
        tgt = { id: to.id, dist, inRange: dist <= rangeOf(), hp: frac(v), hpText: hpa && v !== undefined ? `${hpa.name} ${Math.round(v)}` : '', locked }
      }
      s.target = tgt

      for (let i = floaters.length - 1; i >= 0; i--) {
        const f = floaters[i]!
        f.age += dt
        if (f.age >= FLOAT_LIFE) {
          drop(f)
          floaters.splice(i, 1)
          continue
        }
        f.sprite.position.y = f.y + f.age * 4
        f.sprite.material.opacity = 1 - (f.age / FLOAT_LIFE) ** 2
      }

      if (self) {
        const sx = self.x - half
        const sz = self.y - half
        me.visible = true
        me.position.set(sx, 1.3, sz)
        me.rotation.y = now / 1200
        meCore.visible = mePool.visible = lightPool.visible = true
        meCore.position.copy(me.position)
        meCore.rotation.y = -now / 900
        mePool.position.set(sx, 0.09, sz)
        lightPool.position.set(sx, 0.05, sz)
        if (selfPrev) {
          const mv = Math.hypot(sx - selfPrev.x, sz - selfPrev.z)
          if (mv > 0.02) {
            const k = Math.min(4, 1 + Math.floor(mv * 14))
            for (let i = 0; i < k; i++) {
              const t = Math.random()
              sparks.emit(sx - (sx - selfPrev.x) * t + (Math.random() - 0.5) * 0.6, 0.4 + Math.random() * 0.8, sz - (sz - selfPrev.z) * t + (Math.random() - 0.5) * 0.6, (Math.random() - 0.5) * 1.2, 0.5 + Math.random() * 1.5, (Math.random() - 0.5) * 1.2, 0.55 + Math.random() * 0.4, 0.5, cHot, 0.8, 0, 1.2)
            }
          }
        }
        selfPrev = { x: sx, z: sz }
        const k = clampZoom(s.zoom)
        const h = 34 * k
        const back = 30 * k
        if (!camReady) {
          cam.set(sx, h, sz + back)
          camReady = true
        }
        const f = 1 - Math.exp(-dt * 10)
        cam.x += (sx - cam.x) * f
        cam.z += (sz + back - cam.z) * f
        cam.y += (h - cam.y) * f
        shake *= Math.exp(-dt * 7)
        grid.position.set(Math.round(sx / STEP) * STEP, 0, Math.round(sz / STEP) * STEP)
        camera.position.copy(cam)
        if (shake > 0.01) camera.position.add(shakeV.set((Math.random() - 0.5) * shake, (Math.random() - 0.5) * shake, (Math.random() - 0.5) * shake))
        camLook.set(sx, 0, sz)
        camera.lookAt(camLook)
        ring.visible = !!tgt
        beam.visible = !!tgt
        if (tgt) {
          beam.position.set(tx, 9, tz)
          ;(beam.material as THREE.MeshBasicMaterial).opacity = (tgt.inRange ? 0.55 : 0.2) + Math.sin(now / 200) * 0.1
          if (Math.random() < 0.5) sparks.emit(tx + Math.cos(now / 300) * 1.6, 0.2, tz + Math.sin(now / 300) * 1.6, 0, 3 + Math.random() * 2, 0, 0.6, 0.4, cAcc, 0.8, 0, 0)
          ring.position.set(tx, 0.2, tz)
          ringMat.color.setHex(tgt.inRange ? pal.accent : pal.dim)
          const sc = tgt.locked ? 1.25 + Math.sin(now / 160) * 0.08 : 1
          ring.scale.set(sc, 1, sc)
        }
      } else {
        me.visible = false
        meCore.visible = mePool.visible = lightPool.visible = false
        ring.visible = false
        beam.visible = false
        selfPrev = null
      }
      const wx = s.weather
      if (wx && self) {
        const k = Math.max(0, Math.min(1, wx.intensity))
        fogTint.set(wx.color)
        scene.background = null
        fog.color.copy(fogBase).lerp(fogTint, 0.35 * k)
        renderer.setClearColor(fog.color, 1)
        fog.near = 50 - 40 * k
        fog.far = 150 - 90 * k
        rainMat.color.set(wx.color).lerp(new THREE.Color(pal.line), 0.4)
        const n = k > 0.25 ? Math.round(RAIN * k) : 0
        rain.visible = n > 0
        const rp = rainGeo.getAttribute('position') as THREE.BufferAttribute
        const sx = self.x - half
        const sz = self.y - half
        for (let i = 0; i < n; i++) {
          const r = rainPos[i]!
          r.y -= dt * 38
          if (r.y < 0) {
            r.y = 40
            r.x = Math.random() * 80 - 40
            r.z = Math.random() * 80 - 40
          }
          rp.setXYZ(i * 2, sx + r.x, r.y, sz + r.z)
          rp.setXYZ(i * 2 + 1, sx + r.x, r.y + 1.6, sz + r.z)
        }
        rainGeo.setDrawRange(0, n * 2)
        rp.needsUpdate = true
      } else {
        rain.visible = false
        fog.color.copy(fogBase)
        renderer.setClearColor(fogBase, 1)
        fog.near = 50
        fog.far = 150
      }
      for (let i = waves.length - 1; i >= 0; i--) {
        const w = waves[i]!
        w.age += dt
        const t = w.age / w.max
        if (t >= 1) {
          scene.remove(w.mesh)
          ;(w.mesh.material as THREE.Material).dispose()
          waves.splice(i, 1)
          continue
        }
        const r = 1 + (w.size - 1) * (1 - (1 - t) ** 3)
        w.mesh.scale.set(r, 1, r)
        const m = w.mesh.material as THREE.MeshBasicMaterial
        m.opacity = (1 - t) * 0.9
      }
      sparks.update(dt)
      const mx = self ? self.x - half : 0
      const mz = self ? self.y - half : 0
      motes.drift(now / 1000, mx, mz, MOTE_SPAN, MOTE_H, dark ? 0.7 : 0.5)
      if (post) composer.render(dt)
      else renderer.render(scene, camera)
    }
    raf = requestAnimationFrame(frame)

    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
      cv.removeEventListener('pointerdown', onDown)
      cv.removeEventListener('wheel', onWheel)
      for (const f of floaters) drop(f)
      for (const t of textures.values()) t.dispose()
      for (const g of geoms) g.dispose()
      for (const d of disposables) d.dispose()
      for (const w of waves) (w.mesh.material as THREE.Material).dispose()
      sparks.dispose()
      motes.dispose()
      composer.dispose()
      target.dispose()
      grid.geometry.dispose()
      ;(grid.material as THREE.Material).dispose()
      ringMat.dispose()
      rainGeo.dispose()
      rainMat.dispose()
      for (const o of [others, cores, pools, gems, gemPools, me, ring, bars]) {
        o.geometry.dispose()
        ;(o.material as THREE.Material).dispose()
      }
      renderer.dispose()
      cv.remove()
    }
  }, [info, dark])

  return <div ref={box} className="absolute inset-0 overflow-hidden" style={{ background: 'var(--hud-bg)' }} />
}
