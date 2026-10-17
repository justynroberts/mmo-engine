import { useEffect, useRef, useState, type MutableRefObject } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { KIND_DRIVEN, KIND_PLAYER, type Place, type Route, type State, type WorldGeom } from '../api'

interface Props {
  world: WorldGeom | null
  places: Place[]
  routes: Route[]
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

interface Ghost {
  x: number
  z: number
  tx: number
  tz: number
  kind: number
  arch: number
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
  const props = useRef(p)
  props.current = p
  const [failed, setFailed] = useState(false)
  const [count, setCount] = useState(0)
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

    const wire = (c: number, o = 1) => new THREE.MeshBasicMaterial({ color: c, wireframe: true, transparent: o < 1, opacity: o })
    const bots = new THREE.InstancedMesh(new THREE.OctahedronGeometry(4), wire(0xffffff), CAP)
    const people = new THREE.InstancedMesh(new THREE.ConeGeometry(3.4, 11, 4), wire(0xffffff), CAP)
    for (const m of [bots, people]) {
      m.instanceMatrix.setUsage(THREE.DynamicDrawUsage)
      m.frustumCulled = false
      m.count = 0
      scene.add(m)
    }
    const tmp = new THREE.Object3D()
    const col = new THREE.Color()

    // Static layers are rebuilt when the world, places or routes change.
    const statics = new THREE.Group()
    scene.add(statics)
    let builtFor = ''
    const dispose = (g: THREE.Group) => {
      for (const c of [...g.children]) {
        g.remove(c)
        const o = c as THREE.Mesh
        o.geometry?.dispose()
        const m = o.material as THREE.Material | undefined
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
      const walls: THREE.BufferGeometry[] = []
      for (const [x0, y0, x1, y1] of pr.world?.boxes ?? []) {
        const g = new THREE.EdgesGeometry(new THREE.BoxGeometry(x1 - x0, 8, y1 - y0))
        g.translate((x0 + x1) / 2 - half, 4, (y0 + y1) / 2 - half)
        walls.push(g)
      }
      for (const g of walls) statics.add(new THREE.LineSegments(g, new THREE.LineBasicMaterial({ color: GREEN })))
      for (const pl of pr.places) {
        const ring = new THREE.RingGeometry(Math.max(0.5, pl.r - 0.6), pl.r, 48)
        ring.rotateX(-Math.PI / 2)
        const m = new THREE.Mesh(ring, new THREE.MeshBasicMaterial({ color: HOT, wireframe: true, side: THREE.DoubleSide }))
        m.position.set(pl.x - half, 0.2, pl.y - half)
        statics.add(m)
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
    const botIds: number[] = []
    const peopleIds: number[] = []
    const ray = new THREE.Raycaster()
    ray.params.Points = { threshold: 2 }

    // Click selects; a drag that moved more than a few pixels is an orbit, not a click.
    let down: { x: number; y: number } | null = null
    const onDown = (e: PointerEvent) => (down = { x: e.clientX, y: e.clientY })
    const onUp = (e: PointerEvent) => {
      if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 4) return
      const r = cv.getBoundingClientRect()
      ray.setFromCamera(new THREE.Vector2(((e.clientX - r.left) / r.width) * 2 - 1, -(((e.clientY - r.top) / r.height) * 2 - 1)), camera)
      const hit = (m: THREE.InstancedMesh, ids: number[]) => {
        const h = ray.intersectObject(m)[0]
        return h && h.instanceId !== undefined ? { id: ids[h.instanceId], d: h.distance } : null
      }
      const a = hit(bots, botIds)
      const b = hit(people, peopleIds)
      const best = a && b ? (a.d < b.d ? a : b) : (a ?? b)
      props.current.onSelect(best?.id ?? null)
    }
    cv.addEventListener('pointerdown', onDown)
    cv.addEventListener('pointerup', onUp)

    let raf = 0
    let last = performance.now()
    let shown = -1
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
        for (const row of s.e) {
          const id = row[0] ?? 0
          seen.add(id)
          const tx = (row[1] ?? 0) - half
          const tz = (row[2] ?? 0) - half
          const g = ghosts.get(id)
          if (g) {
            g.tx = tx
            g.tz = tz
            g.kind = row[4] ?? 0
            g.arch = row[3] ?? -1
          } else ghosts.set(id, { x: tx, z: tz, tx, tz, kind: row[4] ?? 0, arch: row[3] ?? -1 })
        }
        for (const id of ghosts.keys()) if (!seen.has(id)) ghosts.delete(id)
      }
      // State arrives ~5 times a second; glide between samples so motion looks continuous.
      const k = 1 - Math.exp(-dt * 12)
      let nb = 0
      let np = 0
      let selPos: Ghost | null = null
      for (const [id, g] of ghosts) {
        g.x += (g.tx - g.x) * k
        g.z += (g.tz - g.z) * k
        if (id === pr.selected) selPos = g
        const person = g.kind === KIND_PLAYER
        if (person ? np >= CAP : nb >= CAP) continue
        const m = person ? people : bots
        const i = person ? np++ : nb++
        ;(person ? peopleIds : botIds)[i] = id
        tmp.position.set(g.x, person ? 5.5 : 4.5, g.z)
        tmp.rotation.set(0, now / (person ? 2400 : 1800) + id, 0)
        tmp.updateMatrix()
        m.setMatrixAt(i, tmp.matrix)
        // Archetypes get different brightness of the same green; players and driven units are near white.
        if (person || g.kind === KIND_DRIVEN) col.setHex(HOT)
        else col.setHSL(0.36, 1, 0.3 + 0.16 * (((g.arch + 1) % 4) / 3))
        m.setColorAt(i, col)
      }
      bots.count = nb
      people.count = np
      for (const m of [bots, people]) {
        m.instanceMatrix.needsUpdate = true
        if (m.instanceColor) m.instanceColor.needsUpdate = true
      }
      if (nb + np !== shown) {
        shown = nb + np
        setCount(shown)
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
    }
    raf = requestAnimationFrame(frame)

    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
      cv.removeEventListener('pointerdown', onDown)
      cv.removeEventListener('pointerup', onUp)
      controls.dispose()
      dispose(statics)
      for (const m of [bots, people, interest, marker]) {
        m.geometry.dispose()
        ;(m.material as THREE.Material).dispose()
      }
      renderer.dispose()
      cv.remove()
    }
  }, [])

  return (
    <div ref={box} className="relative min-h-[240px] h-full bg-black overflow-hidden">
      <canvas ref={rain} aria-hidden="true" className="absolute inset-0 h-full w-full opacity-40" />
      <p className="absolute left-3 bottom-2 m-0 text-[11px] tracking-wider text-[#00ff41] font-mono pointer-events-none" role="status">
        {failed ? 'WebGL is not available in this browser. The 2D map still works.' : `${count} entities · drag to orbit · scroll to zoom`}
      </p>
    </div>
  )
}
