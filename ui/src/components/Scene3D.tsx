import { useEffect, useRef, useState, type MutableRefObject } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { KIND_DRIVEN, KIND_PLAYER, type Place, type Route, type State, type WorldGeom } from '../api'

interface Props {
  world: WorldGeom | null
  places: Place[]
  routes: Route[]
  /** Archetype names, in index order: they decide the shapes and the legend. */
  archetypes: string[]
  /** Index of the health attribute among the replicated ones in an entity row, or -1. */
  healthIdx: number
  healthMax: number
  stateRef: MutableRefObject<State | null>
  selected: number | null
  onSelect: (id: number | null) => void
  follow: boolean
  interest: number
}

// Fixed phosphor palette: the 3D view is a wireframe "terminal" whatever the page theme is.
const GREEN = 0x00ff41
const DIM = 0x0a5a1e
const HOT = 0xd8ffe0
const CAP = 6000

/** One silhouette per archetype (repeating after five), so kinds can be told apart without colour. */
const SHAPES: { name: string; make: () => THREE.BufferGeometry; svg: string }[] = [
  { name: 'diamond', make: () => new THREE.OctahedronGeometry(4), svg: 'M8 1 L15 8 L8 15 L1 8 Z' },
  { name: 'square', make: () => new THREE.BoxGeometry(5.6, 5.6, 5.6), svg: 'M2 2 H14 V14 H2 Z' },
  { name: 'triangle', make: () => new THREE.TetrahedronGeometry(4.6), svg: 'M8 1.5 L15 14 H1 Z' },
  { name: 'ball', make: () => new THREE.IcosahedronGeometry(4, 0), svg: 'M8 1 A7 7 0 1 1 7.99 1 Z' },
  { name: 'hex', make: () => new THREE.CylinderGeometry(4, 4, 5, 6), svg: 'M4.5 2 H11.5 L15 8 L11.5 14 H4.5 L1 8 Z' },
]

interface Ghost {
  x: number
  z: number
  tx: number
  tz: number
  kind: number
  arch: number
  hp: number
}

/** A name on the ground: a sprite textured from a canvas, so no extra font loader is needed. */
function label(text: string): THREE.Sprite {
  const cv = document.createElement('canvas')
  const ctx = cv.getContext('2d')!
  const font = '500 30px "Martian Mono", ui-monospace, monospace'
  ctx.font = font
  const w = Math.ceil(ctx.measureText(text).width) + 16
  cv.width = w
  cv.height = 44
  ctx.font = font
  ctx.fillStyle = '#00ff41'
  ctx.textBaseline = 'middle'
  ctx.fillText(text, 8, 24)
  const tex = new THREE.CanvasTexture(cv)
  tex.colorSpace = THREE.SRGBColorSpace
  const sp = new THREE.Sprite(new THREE.SpriteMaterial({ map: tex, transparent: true, depthWrite: false, fog: false }))
  const h = 12
  sp.scale.set((w / 44) * h, h, 1)
  return sp
}

/** A falling-digits backdrop. Static when the user prefers reduced motion. */
function useRain(cv: React.RefObject<HTMLCanvasElement>) {
  useEffect(() => {
    const el = cv.current
    const ctx = el?.getContext('2d')
    if (!el || !ctx) return
    const still = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
    const cell = 16
    let drops: number[] = []
    const fit = () => {
      el.width = el.clientWidth
      el.height = el.clientHeight
      drops = Array.from({ length: Math.ceil(el.width / cell) }, () => Math.random() * (el.height / cell))
      ctx.fillStyle = '#000'
      ctx.fillRect(0, 0, el.width, el.height)
    }
    fit()
    const ro = new ResizeObserver(fit)
    ro.observe(el)
    const step = () => {
      ctx.fillStyle = 'rgba(0,0,0,0.12)'
      ctx.fillRect(0, 0, el.width, el.height)
      ctx.font = `${cell - 2}px "Martian Mono", monospace`
      ctx.fillStyle = '#0f7a2c'
      drops.forEach((y, i) => {
        ctx.fillText(String((Math.random() * 10) | 0), i * cell, y * cell)
        drops[i] = y * cell > el.height && Math.random() > 0.975 ? 0 : y + 1
      })
    }
    if (still) for (let k = 0; k < 60; k++) step()
    const t = still ? undefined : setInterval(step, 70)
    return () => {
      ro.disconnect()
      if (t) clearInterval(t)
    }
  }, [cv])
}

export default function Scene3D(p: Props) {
  const box = useRef<HTMLDivElement>(null)
  const rain = useRef<HTMLCanvasElement>(null)
  const tag = useRef<HTMLDivElement>(null)
  const props = useRef(p)
  props.current = p
  const [failed, setFailed] = useState(false)
  const [counts, setCounts] = useState<number[]>([])
  const [players, setPlayers] = useState(0)
  useRain(rain)

  useEffect(() => {
    const host = box.current
    if (!host) return
    let renderer: THREE.WebGLRenderer
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true })
    } catch {
      setFailed(true)
      return
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2))
    renderer.setClearColor(0x000000, 0)
    const cv = renderer.domElement
    cv.className = 'absolute inset-0 h-full w-full touch-none'
    cv.setAttribute('role', 'img')
    cv.setAttribute('aria-label', '3D wireframe view of the world. Drag to orbit, scroll to zoom, click an entity to select it. The Entity tab lists the same entities.')
    host.appendChild(cv)

    const scene = new THREE.Scene()
    const camera = new THREE.PerspectiveCamera(50, 1, 1, 8000)
    const controls = new OrbitControls(camera, cv)
    controls.enableDamping = true
    controls.maxPolarAngle = Math.PI / 2 - 0.05
    controls.minDistance = 10

    const wire = (c: number) => new THREE.MeshBasicMaterial({ color: c, wireframe: true })
    // One instanced mesh per silhouette, plus one for players.
    const kinds = SHAPES.map((s) => {
      const m = new THREE.InstancedMesh(s.make(), wire(0xffffff), CAP)
      m.instanceMatrix.setUsage(THREE.DynamicDrawUsage)
      m.frustumCulled = false
      m.count = 0
      scene.add(m)
      return { mesh: m, ids: [] as number[] }
    })
    const people = new THREE.InstancedMesh(new THREE.ConeGeometry(3.4, 11, 4), wire(0xffffff), CAP)
    people.instanceMatrix.setUsage(THREE.DynamicDrawUsage)
    people.frustumCulled = false
    people.count = 0
    scene.add(people)
    const peopleIds: number[] = []
    const meshes = [...kinds.map((k) => k.mesh), people]
    const tmp = new THREE.Object3D()
    const col = new THREE.Color()

    // Health bars for anything hurt: two segments each, in a fixed buffer rewritten every frame.
    const bars = new THREE.LineSegments(
      new THREE.BufferGeometry().setAttribute('position', new THREE.BufferAttribute(new Float32Array(CAP * 12), 3)),
      new THREE.LineBasicMaterial({ color: HOT }),
    )
    bars.frustumCulled = false
    scene.add(bars)

    // Static layers are rebuilt when the world, places or routes change.
    const statics = new THREE.Group()
    scene.add(statics)
    let builtFor = ''
    const dispose = (g: THREE.Group) => {
      for (const c of [...g.children]) {
        g.remove(c)
        const o = c as THREE.Mesh
        o.geometry?.dispose()
        const m = o.material as THREE.Material & { map?: THREE.Texture }
        m?.map?.dispose()
        m?.dispose()
      }
    }
    let half = 1024
    const build = () => {
      const pr = props.current
      const size = pr.world?.size ?? 2048
      const key = JSON.stringify([size, pr.world?.boxes.length, pr.places, pr.routes])
      if (key === builtFor) return
      builtFor = key
      dispose(statics)
      half = size / 2
      scene.fog = new THREE.Fog(0x000000, size * 0.35, size * 1.4)
      const grid = new THREE.GridHelper(size, Math.max(8, Math.round(size / 64)), GREEN, DIM)
      ;(grid.material as THREE.Material).transparent = true
      ;(grid.material as THREE.Material).opacity = 0.55
      statics.add(grid)
      for (const [x0, y0, x1, y1] of pr.world?.boxes ?? []) {
        const g = new THREE.EdgesGeometry(new THREE.BoxGeometry(x1 - x0, 8, y1 - y0))
        g.translate((x0 + x1) / 2 - half, 4, (y0 + y1) / 2 - half)
        statics.add(new THREE.LineSegments(g, new THREE.LineBasicMaterial({ color: GREEN })))
      }
      for (const pl of pr.places) {
        const ring = new THREE.RingGeometry(Math.max(0.5, pl.r - 0.6), pl.r, 48)
        ring.rotateX(-Math.PI / 2)
        const m = new THREE.Mesh(ring, new THREE.MeshBasicMaterial({ color: HOT, wireframe: true, side: THREE.DoubleSide }))
        m.position.set(pl.x - half, 0.2, pl.y - half)
        statics.add(m)
        const t = label(pl.name)
        t.position.set(pl.x - half, 22 + pl.r * 0.2, pl.y - half)
        statics.add(t)
      }
      for (const rt of pr.routes) {
        const pts = rt.points.map(([x, y]) => new THREE.Vector3(x - half, 0.4, y - half))
        if (rt.loop && pts.length > 2) pts.push(pts[0]!.clone())
        if (pts.length > 1) statics.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(pts), new THREE.LineDashedMaterial({ color: GREEN, dashSize: 6, gapSize: 4 })))
      }
      for (const o of statics.children) if (o instanceof THREE.Line) o.computeLineDistances()
      camera.far = size * 3
      camera.updateProjectionMatrix()
    }

    // Selection marker: an interest-radius circle plus a spinning diamond.
    const interest = new THREE.Mesh(new THREE.RingGeometry(0.985, 1, 96).rotateX(-Math.PI / 2), new THREE.MeshBasicMaterial({ color: HOT, side: THREE.DoubleSide, transparent: true, opacity: 0.6 }))
    const marker = new THREE.Mesh(new THREE.OctahedronGeometry(5), new THREE.MeshBasicMaterial({ color: HOT, wireframe: true }))
    interest.visible = marker.visible = false
    scene.add(interest, marker)

    let fitted = 0
    const fit = () => {
      const w = Math.max(50, host.clientWidth)
      const h = Math.max(50, host.clientHeight)
      renderer.setSize(w, h, false)
      camera.aspect = w / h
      camera.updateProjectionMatrix()
    }
    fit()
    const ro = new ResizeObserver(fit)
    ro.observe(host)

    const ghosts = new Map<number, Ghost>()
    const ray = new THREE.Raycaster()

    // Click selects; a drag that moved more than a few pixels is an orbit, not a click.
    let down: { x: number; y: number } | null = null
    const onDown = (e: PointerEvent) => (down = { x: e.clientX, y: e.clientY })
    const onUp = (e: PointerEvent) => {
      if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 4) return
      const r = cv.getBoundingClientRect()
      ray.setFromCamera(new THREE.Vector2(((e.clientX - r.left) / r.width) * 2 - 1, -(((e.clientY - r.top) / r.height) * 2 - 1)), camera)
      let best: { id: number | undefined; d: number } | null = null
      const consider = (m: THREE.InstancedMesh, ids: number[]) => {
        const h = ray.intersectObject(m)[0]
        if (h && h.instanceId !== undefined && (!best || h.distance < best.d)) best = { id: ids[h.instanceId], d: h.distance }
      }
      for (const k of kinds) consider(k.mesh, k.ids)
      consider(people, peopleIds)
      props.current.onSelect((best as { id: number | undefined } | null)?.id ?? null)
    }
    cv.addEventListener('pointerdown', onDown)
    cv.addEventListener('pointerup', onUp)

    let raf = 0
    let last = performance.now()
    let lastShown = ''
    const vec = new THREE.Vector3()
    const frame = (now: number) => {
      raf = requestAnimationFrame(frame)
      const dt = Math.min(0.1, (now - last) / 1000)
      last = now
      const pr = props.current
      build()
      if (fitted !== half) {
        fitted = half
        camera.position.set(half * 0.55, half * 0.5, half * 0.55)
        controls.target.set(0, 0, 0)
      }
      const s = pr.stateRef.current
      if (s) {
        const seen = new Set<number>()
        const hi = pr.healthIdx
        for (const row of s.e) {
          const id = row[0] ?? 0
          seen.add(id)
          const tx = (row[1] ?? 0) - half
          const tz = (row[2] ?? 0) - half
          const hp = hi >= 0 && pr.healthMax > 0 ? (row[5 + hi] ?? pr.healthMax) / pr.healthMax : 1
          const g = ghosts.get(id)
          if (g) {
            g.tx = tx
            g.tz = tz
            g.kind = row[4] ?? 0
            g.arch = row[3] ?? -1
            g.hp = hp
          } else ghosts.set(id, { x: tx, z: tz, tx, tz, kind: row[4] ?? 0, arch: row[3] ?? -1, hp })
        }
        for (const id of ghosts.keys()) if (!seen.has(id)) ghosts.delete(id)
      }
      // State arrives ~5 times a second; glide between samples so motion looks continuous.
      const k = 1 - Math.exp(-dt * 12)
      const n = new Array<number>(SHAPES.length).fill(0)
      const per = new Array<number>(Math.max(pr.archetypes.length, 1)).fill(0)
      let np = 0
      let nb = 0
      let selPos: Ghost | null = null
      const bp = bars.geometry.getAttribute('position') as THREE.BufferAttribute
      for (const [id, g] of ghosts) {
        g.x += (g.tx - g.x) * k
        g.z += (g.tz - g.z) * k
        if (id === pr.selected) selPos = g
        const person = g.kind === KIND_PLAYER
        const shape = person ? -1 : (g.arch < 0 ? 0 : g.arch) % SHAPES.length
        if (person ? np >= CAP : n[shape]! >= CAP) continue
        const mesh = person ? people : kinds[shape]!.mesh
        const i = person ? np++ : n[shape]!++
        ;(person ? peopleIds : kinds[shape]!.ids)[i] = id
        if (!person && g.arch >= 0 && g.arch < per.length) per[g.arch]!++
        tmp.position.set(g.x, person ? 5.5 : 4.5, g.z)
        tmp.rotation.set(0, now / (person ? 2400 : 1800) + id, 0)
        tmp.updateMatrix()
        mesh.setMatrixAt(i, tmp.matrix)
        // Archetypes also differ in brightness; players and driven units are near white.
        if (person || g.kind === KIND_DRIVEN) col.setHex(HOT)
        else col.setHSL(0.36, 1, 0.3 + 0.16 * (((g.arch + 1) % 4) / 3))
        mesh.setColorAt(i, col)
        if (g.hp < 0.995 && nb < CAP) {
          const w = 12
          const y = person ? 13 : 11
          const f = Math.max(0, Math.min(1, g.hp))
          bp.setXYZ(nb * 4, g.x - w / 2, y, g.z)
          bp.setXYZ(nb * 4 + 1, g.x - w / 2 + w * f, y, g.z)
          bp.setXYZ(nb * 4 + 2, g.x - w / 2, y + 0.4, g.z)
          bp.setXYZ(nb * 4 + 3, g.x - w / 2 + w * f, y + 0.4, g.z)
          nb++
        }
      }
      kinds.forEach((kd, i) => (kd.mesh.count = n[i]!))
      people.count = np
      for (const m of meshes) {
        m.instanceMatrix.needsUpdate = true
        if (m.instanceColor) m.instanceColor.needsUpdate = true
      }
      bars.geometry.setDrawRange(0, nb * 4)
      bp.needsUpdate = true
      const sig = per.join(',') + '/' + np
      if (sig !== lastShown) {
        lastShown = sig
        setCounts(per)
        setPlayers(np)
      }
      if (selPos) {
        interest.visible = marker.visible = true
        interest.position.set(selPos.x, 0.3, selPos.z)
        interest.scale.setScalar(pr.interest)
        marker.position.set(selPos.x, 18, selPos.z)
        marker.rotation.y = now / 600
        if (pr.follow) controls.target.lerp(new THREE.Vector3(selPos.x, 0, selPos.z), 0.15)
      } else interest.visible = marker.visible = false
      controls.update()
      renderer.render(scene, camera)

      // The selected entity's tag follows it on screen.
      const el = tag.current
      if (el) {
        if (selPos && pr.selected !== null) {
          vec.set(selPos.x, 24, selPos.z).project(camera)
          const visible = vec.z < 1 && Math.abs(vec.x) < 1.05 && Math.abs(vec.y) < 1.05
          el.style.display = visible ? 'block' : 'none'
          el.style.transform = `translate(${((vec.x + 1) / 2) * host.clientWidth}px, ${((1 - vec.y) / 2) * host.clientHeight}px) translate(-50%, -100%)`
          const name = selPos.kind === KIND_PLAYER ? 'player' : (pr.archetypes[selPos.arch] ?? 'entity')
          const hp = pr.healthIdx >= 0 ? ` · ${Math.round(Math.max(0, selPos.hp) * pr.healthMax)}/${pr.healthMax}` : ''
          const text = `${name} #${pr.selected}${hp}`
          if (el.textContent !== text) el.textContent = text
        } else el.style.display = 'none'
      }
    }
    raf = requestAnimationFrame(frame)

    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
      cv.removeEventListener('pointerdown', onDown)
      cv.removeEventListener('pointerup', onUp)
      controls.dispose()
      dispose(statics)
      for (const m of [...meshes, interest, marker, bars]) {
        m.geometry.dispose()
        ;(m.material as THREE.Material).dispose()
      }
      renderer.dispose()
      cv.remove()
    }
  }, [])

  const legend = p.archetypes.map((name, i) => ({ name, i, n: counts[i] ?? 0, shape: SHAPES[i % SHAPES.length]! }))
  return (
    <div ref={box} className="relative min-h-[240px] h-full bg-black overflow-hidden">
      <canvas ref={rain} aria-hidden="true" className="absolute inset-0 h-full w-full opacity-40" />
      <div
        ref={tag}
        aria-hidden="true"
        style={{ display: 'none' }}
        className="absolute left-0 top-0 pointer-events-none whitespace-nowrap border border-[#0a5a1e] bg-black/80 px-1 text-[11px] leading-4 text-[#d8ffe0] font-mono"
      />
      {!failed && (
        <ul aria-label="Legend" className="absolute left-3 bottom-8 m-0 p-0 list-none text-[11px] leading-5 text-[#00ff41] font-mono pointer-events-none">
          {legend.map((l) => (
            <li key={l.name} className="flex items-center gap-2">
              <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d={l.shape.svg} />
              </svg>
              <span>{l.name}</span>
              <span className="opacity-60">{l.n}</span>
            </li>
          ))}
          <li className="flex items-center gap-2 text-[#d8ffe0]">
            <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M8 1 L13 14 H3 Z" />
            </svg>
            <span>players</span>
            <span className="opacity-60">{players}</span>
          </li>
        </ul>
      )}
      <p className="absolute left-3 bottom-2 m-0 text-[11px] tracking-wider text-[#00ff41] font-mono pointer-events-none" role="status">
        {failed ? 'WebGL is not available in this browser. The 2D map still works.' : 'drag to orbit · scroll to zoom · click to select'}
      </p>
    </div>
  )
}
