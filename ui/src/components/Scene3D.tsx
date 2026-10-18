import { useEffect, useRef, useState, type MutableRefObject } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { KIND_DRIVEN, KIND_PLAYER, type Place, type Route, type State, type WorldGeom } from '../api'
import { BLOCK, BLOCK_COLOURS, MAP, OPEN, TREES, TYPE_COLOUR, groundColour, hpColor, type BlockCol } from './mapLook'
import { fromBoxes, type WorldData } from '../world'

interface Props {
  world: WorldGeom | null
  places: Place[]
  routes: Route[]
  /** Archetype names, in index order: they decide the shapes and the legend. */
  archetypes: string[]
  /** Archetype colours, in the same order; entities are painted with them. */
  colors?: string[]
  /** The world file, when the server has one: heights, water and objects to draw. Without it, the walls. */
  land?: WorldData | null
  /** Blocks players have built. */
  blocks?: BlockCol[]
  /** Index of the health attribute among the replicated ones in an entity row, or -1. */
  healthIdx: number
  healthMax: number
  /** Index of the `airborne` attribute in an entity row's attributes, or -1; jumping entities are lifted by it. */
  airIdx?: number
  airMax?: number
  stateRef: MutableRefObject<State | null>
  selected: number | null
  onSelect: (id: number | null) => void
  follow: boolean
  interest: number
}

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
  air: number
}

/** A name over a place: a sprite textured from a canvas, parchment with an ink rim so it reads on any ground. */
function label(text: string): THREE.Sprite {
  const cv = document.createElement('canvas')
  const ctx = cv.getContext('2d')!
  const font = '500 30px "Martian Mono", ui-monospace, monospace'
  ctx.font = font
  const w = Math.ceil(ctx.measureText(text).width) + 16
  cv.width = w
  cv.height = 44
  ctx.font = font
  ctx.textBaseline = 'middle'
  ctx.lineWidth = 6
  ctx.lineJoin = 'round'
  ctx.strokeStyle = MAP.ink
  ctx.strokeText(text, 8, 24)
  ctx.fillStyle = MAP.parchment
  ctx.fillText(text, 8, 24)
  const tex = new THREE.CanvasTexture(cv)
  tex.colorSpace = THREE.SRGBColorSpace
  const sp = new THREE.Sprite(new THREE.SpriteMaterial({ map: tex, transparent: true, depthWrite: false, fog: false }))
  const h = 12
  sp.scale.set((w / 44) * h, h, 1)
  return sp
}

export default function Scene3D(p: Props) {
  const box = useRef<HTMLDivElement>(null)
  const tag = useRef<HTMLDivElement>(null)
  const props = useRef(p)
  props.current = p
  const [failed, setFailed] = useState(false)
  const [counts, setCounts] = useState<number[]>([])
  const [players, setPlayers] = useState(0)

  useEffect(() => {
    const host = box.current
    if (!host) return
    let renderer: THREE.WebGLRenderer
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true })
    } catch {
      setFailed(true)
      return
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2))
    renderer.setClearColor(MAP.sky)
    const cv = renderer.domElement
    cv.className = 'absolute inset-0 h-full w-full touch-none'
    cv.setAttribute('role', 'img')
    cv.setAttribute('aria-label', '3D view of the world. Drag to orbit, scroll to zoom, click an entity to select it. The Entity tab lists the same entities.')
    host.appendChild(cv)

    const scene = new THREE.Scene()
    scene.background = new THREE.Color(MAP.sky)
    scene.add(new THREE.HemisphereLight(0xcfe0ee, 0x4a5a2a, 1.1))
    const sun = new THREE.DirectionalLight(0xfff1d6, 1.6)
    sun.position.set(-0.6, 1, -0.4)
    scene.add(sun)
    const camera = new THREE.PerspectiveCamera(50, 1, 1, 8000)
    const controls = new OrbitControls(camera, cv)
    controls.enableDamping = true
    controls.maxPolarAngle = Math.PI / 2 - 0.05
    controls.minDistance = 10

    const solid = () => new THREE.MeshLambertMaterial({ color: 0xffffff, flatShading: true })
    // One instanced mesh per silhouette, plus one for players.
    const kinds = SHAPES.map((s) => {
      const m = new THREE.InstancedMesh(s.make(), solid(), CAP)
      m.instanceMatrix.setUsage(THREE.DynamicDrawUsage)
      m.frustumCulled = false
      m.count = 0
      scene.add(m)
      return { mesh: m, ids: [] as number[] }
    })
    const people = new THREE.InstancedMesh(new THREE.ConeGeometry(3.4, 11, 4), solid(), CAP)
    people.instanceMatrix.setUsage(THREE.DynamicDrawUsage)
    people.frustumCulled = false
    people.count = 0
    scene.add(people)
    const peopleIds: number[] = []
    const meshes = [...kinds.map((k) => k.mesh), people]
    const tmp = new THREE.Object3D()
    const col = new THREE.Color()

    // Health bars for anything hurt: two segments each, coloured by how hurt, in a buffer rewritten every frame.
    const barGeo = new THREE.BufferGeometry()
    barGeo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(CAP * 12), 3))
    barGeo.setAttribute('color', new THREE.BufferAttribute(new Float32Array(CAP * 12), 3))
    const bars = new THREE.LineSegments(barGeo, new THREE.LineBasicMaterial({ vertexColors: true }))
    bars.frustumCulled = false
    scene.add(bars)

    // Static layers are rebuilt when the world, places or routes change.
    const statics = new THREE.Group()
    scene.add(statics)
    let builtFor = ''
    // Built blocks: one instanced cube per material, rebuilt when the list (or the land under it) changes.
    const blockGroup = new THREE.Group()
    scene.add(blockGroup)
    let blocksFor: [BlockCol[] | undefined, WorldData | null] = [undefined, null]
    const buildBlocks = () => {
      const pr = props.current
      if (blocksFor[0] === pr.blocks && blocksFor[1] === land) return
      blocksFor = [pr.blocks, land]
      dispose(blockGroup)
      const at: THREE.Vector3[][] = [[], [], [], []]
      for (const [cx, cy, kinds] of pr.blocks ?? []) {
        const x0 = cx * BLOCK
        const y0 = cy * BLOCK
        const base = Math.min(land.height(x0, y0), land.height(x0 + BLOCK, y0), land.height(x0, y0 + BLOCK), land.height(x0 + BLOCK, y0 + BLOCK))
        for (let i = 0; i < kinds.length; i++) at[Number(kinds[i]) & 3]!.push(new THREE.Vector3(x0 + BLOCK / 2 - half, base + (i + 0.5) * BLOCK, y0 + BLOCK / 2 - half))
      }
      at.forEach((list, k) => {
        if (!list.length) return
        const mesh = new THREE.InstancedMesh(new THREE.BoxGeometry(BLOCK, BLOCK, BLOCK), new THREE.MeshLambertMaterial({ color: BLOCK_COLOURS[k] }), list.length)
        list.forEach((p, i) => {
          tmp.position.copy(p)
          tmp.rotation.set(0, 0, 0)
          tmp.scale.setScalar(1)
          tmp.updateMatrix()
          mesh.setMatrixAt(i, tmp.matrix)
        })
        blockGroup.add(mesh)
      })
    }
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
    let land: WorldData = fromBoxes(2048, [])
    const ground = (x: number, z: number) => land.height(x + half, z + half)
    // How tall each kind of footprint stands, for a block drawing of it; crags and lakes are in the heights.
    const TALL: Record<string, number> = { house: 9, keep: 8, castle: 14, tower: 14, wall: 7, block: 8 }
    let builtLand: WorldData | null = null
    const build = () => {
      const pr = props.current
      const size = pr.world?.size ?? 2048
      const next = pr.land ?? fromBoxes(size, pr.world?.boxes ?? [])
      const key = JSON.stringify([size, pr.world?.boxes.length, pr.places, pr.routes])
      if (key === builtFor && (pr.land ? builtLand === pr.land : true)) return
      builtFor = key
      builtLand = pr.land ?? null
      land = next
      dispose(statics)
      half = size / 2
      scene.fog = new THREE.Fog(MAP.fog, size * 0.35, size * 1.4)

      // Ground from the world's heights, coloured by height, over a dark apron past the edge.
      const seg = Math.min(220, Math.max(64, Math.round(size / 10)))
      const g = new THREE.PlaneGeometry(size, size, seg, seg).rotateX(-Math.PI / 2)
      const pos = g.getAttribute('position') as THREE.BufferAttribute
      const colors = new Float32Array(pos.count * 3)
      const c = new THREE.Color()
      let low = 0
      for (let i = 0; i < pos.count; i++) {
        const x = pos.getX(i) + half
        const z = pos.getZ(i) + half
        const h = land.height(x, z)
        low = Math.min(low, h)
        pos.setY(i, h)
        const [r, gg, b] = groundColour(h)
        c.setRGB(r, gg, b, THREE.SRGBColorSpace)
        colors.set([c.r, c.g, c.b], i * 3)
      }
      g.setAttribute('color', new THREE.BufferAttribute(colors, 3))
      g.computeVertexNormals()
      statics.add(new THREE.Mesh(g, new THREE.MeshLambertMaterial({ vertexColors: true })))
      const apron = new THREE.Mesh(new THREE.PlaneGeometry(size * 5, size * 5).rotateX(-Math.PI / 2), new THREE.MeshLambertMaterial({ color: MAP.void }))
      apron.position.y = low - 2
      statics.add(apron)

      // Footprints as blocks coloured by type, with a dark rim so their shape reads from far off.
      for (const o of land.objects) {
        if (!o.b || OPEN.has(o.t) || o.t === 'crag' || o.t === 'lake') continue
        const [x0, y0, x1, y1] = o.b
        const tall = TALL[o.t] ?? 8
        const bg = new THREE.BoxGeometry(x1 - x0, tall, y1 - y0)
        bg.translate((x0 + x1) / 2 - half, (o.y ?? 0) + tall / 2, (y0 + y1) / 2 - half)
        statics.add(new THREE.Mesh(bg, new THREE.MeshLambertMaterial({ color: TYPE_COLOUR[o.t] ?? MAP.stone, flatShading: true })))
        statics.add(new THREE.LineSegments(new THREE.EdgesGeometry(bg), new THREE.LineBasicMaterial({ color: MAP.stoneEdge })))
      }
      // Water: lakes at their level, rivers as a ribbon at the water's height.
      const waterMat = new THREE.MeshLambertMaterial({ color: MAP.water, transparent: true, opacity: 0.85, side: THREE.DoubleSide })
      for (const l of land.lakes) {
        const [x0, y0, x1, y1] = l.b
        const lake = new THREE.PlaneGeometry(x1 - x0, y1 - y0).rotateX(-Math.PI / 2)
        lake.translate((x0 + x1) / 2 - half, l.level, (y0 + y1) / 2 - half)
        statics.add(new THREE.Mesh(lake, waterMat))
      }
      for (const r of land.rivers) {
        const pos: number[] = []
        r.pts.forEach((p, k) => {
          const a = r.pts[Math.max(0, k - 1)]!
          const b = r.pts[Math.min(r.pts.length - 1, k + 1)]!
          const len = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1
          const nx = (-(b[1] - a[1]) / len) * (r.half_width + 1)
          const nz = ((b[0] - a[0]) / len) * (r.half_width + 1)
          pos.push(p[0] + nx - half, p[2], p[1] + nz - half, p[0] - nx - half, p[2], p[1] - nz - half)
        })
        const idx: number[] = []
        for (let k = 1; k < r.pts.length; k++) {
          const i = k * 2
          idx.push(i - 2, i - 1, i, i - 1, i + 1, i)
        }
        const rg = new THREE.BufferGeometry()
        rg.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3))
        rg.setIndex(idx)
        rg.computeVertexNormals()
        statics.add(new THREE.Mesh(rg, waterMat))
      }

      // Trees: a trunk and a canopy each, instanced (at most 20000, nearest the middle first if there are more).
      const ts = land.objects.filter((o) => o.p && TREES.has(o.t)).slice(0, 20000)
      const trunks = new THREE.InstancedMesh(new THREE.CylinderGeometry(0.5, 0.7, 4, 5), new THREE.MeshLambertMaterial({ color: MAP.trunk }), Math.max(1, ts.length))
      const tops = new THREE.InstancedMesh(new THREE.IcosahedronGeometry(3.2, 0), new THREE.MeshLambertMaterial({ color: MAP.tree, flatShading: true }), Math.max(1, ts.length))
      ts.forEach((o, i) => {
        const [x, y] = o.p!
        const s = (o.s ?? 1.6) * (o.t === 'bush' ? 0.8 : 1.6)
        const h = land.height(x, y)
        tmp.position.set(x - half, h + s, y - half)
        tmp.rotation.set(0, i, 0)
        tmp.scale.setScalar(s / 2.5)
        tmp.updateMatrix()
        trunks.setMatrixAt(i, tmp.matrix)
        tmp.position.y = h + s * 2.4
        tmp.updateMatrix()
        tops.setMatrixAt(i, tmp.matrix)
      })
      trunks.count = tops.count = ts.length
      tmp.scale.setScalar(1)
      statics.add(trunks, tops)

      for (const pl of pr.places) {
        const ring = new THREE.RingGeometry(Math.max(0.5, pl.r - 0.8), pl.r, 48)
        ring.rotateX(-Math.PI / 2)
        const m = new THREE.Mesh(ring, new THREE.MeshBasicMaterial({ color: MAP.gold, side: THREE.DoubleSide, transparent: true, opacity: 0.8 }))
        m.position.set(pl.x - half, land.height(pl.x, pl.y) + 0.4, pl.y - half)
        statics.add(m)
        const t = label(pl.name)
        t.position.set(pl.x - half, land.height(pl.x, pl.y) + 22 + pl.r * 0.2, pl.y - half)
        statics.add(t)
      }
      for (const rt of pr.routes) {
        // Follow the ground: points every few units along each leg, lifted just above it.
        const raw = rt.loop && rt.points.length > 2 ? [...rt.points, rt.points[0]!] : rt.points
        const pts: THREE.Vector3[] = []
        for (let k = 0; k < raw.length; k++) {
          const a = raw[k]!
          const b = raw[k + 1]
          const steps = b ? Math.max(1, Math.ceil(Math.hypot(b[0] - a[0], b[1] - a[1]) / 6)) : 1
          for (let j = 0; j < steps; j++) {
            const x = b ? a[0] + ((b[0] - a[0]) * j) / steps : a[0]
            const y = b ? a[1] + ((b[1] - a[1]) * j) / steps : a[1]
            pts.push(new THREE.Vector3(x - half, land.height(x, y) + 0.6, y - half))
          }
        }
        if (pts.length > 1) statics.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(pts), new THREE.LineDashedMaterial({ color: MAP.parchment, dashSize: 6, gapSize: 4 })))
      }
      for (const o of statics.children) if (o instanceof THREE.Line) o.computeLineDistances()
      camera.far = size * 3
      camera.updateProjectionMatrix()
    }

    // Selection marker: an interest-radius circle plus a turning diamond, in gold like the client's target ring.
    const interest = new THREE.Mesh(new THREE.RingGeometry(0.985, 1, 96).rotateX(-Math.PI / 2), new THREE.MeshBasicMaterial({ color: MAP.gold, side: THREE.DoubleSide, transparent: true, opacity: 0.7 }))
    const marker = new THREE.Mesh(new THREE.OctahedronGeometry(5), new THREE.MeshLambertMaterial({ color: MAP.gold, flatShading: true }))
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
      buildBlocks()
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
          const ai = pr.airIdx ?? -1
          const air = ai >= 0 ? Math.max(0, Math.min(1, (row[5 + ai] ?? 0) / (pr.airMax || 100))) : 0
          const g = ghosts.get(id)
          if (g) {
            g.tx = tx
            g.tz = tz
            g.kind = row[4] ?? 0
            g.arch = row[3] ?? -1
            g.hp = hp
            g.air = air
          } else ghosts.set(id, { x: tx, z: tz, tx, tz, kind: row[4] ?? 0, arch: row[3] ?? -1, hp, air })
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
      let selY = 0
      const bp = bars.geometry.getAttribute('position') as THREE.BufferAttribute
      const bc = bars.geometry.getAttribute('color') as THREE.BufferAttribute
      for (const [id, g] of ghosts) {
        g.x += (g.tx - g.x) * k
        g.z += (g.tz - g.z) * k
        // On a jump, `air` is the share still to come: a parabola over the ground.
        const gy = ground(g.x, g.z) + (g.air > 0 ? 4 * 6 * (1 - g.air) * g.air : 0)
        if (id === pr.selected) {
          selPos = g
          selY = gy
        }
        const person = g.kind === KIND_PLAYER
        const shape = person ? -1 : (g.arch < 0 ? 0 : g.arch) % SHAPES.length
        if (person ? np >= CAP : n[shape]! >= CAP) continue
        const mesh = person ? people : kinds[shape]!.mesh
        const i = person ? np++ : n[shape]!++
        ;(person ? peopleIds : kinds[shape]!.ids)[i] = id
        if (!person && g.arch >= 0 && g.arch < per.length) per[g.arch]!++
        tmp.position.set(g.x, gy + (person ? 5.5 : 4.5), g.z)
        tmp.rotation.set(0, now / (person ? 2400 : 1800) + id, 0)
        tmp.updateMatrix()
        mesh.setMatrixAt(i, tmp.matrix)
        // Archetypes wear their own colour; players are parchment, possessed units gold.
        if (person) col.set(MAP.parchment)
        else if (g.kind === KIND_DRIVEN) col.set(MAP.gold)
        else col.set(pr.colors?.[g.arch] ?? MAP.parchment)
        mesh.setColorAt(i, col)
        if (g.hp < 0.995 && nb < CAP) {
          const w = 12
          const y = gy + (person ? 13 : 11)
          const f = Math.max(0, Math.min(1, g.hp))
          bp.setXYZ(nb * 4, g.x - w / 2, y, g.z)
          bp.setXYZ(nb * 4 + 1, g.x - w / 2 + w * f, y, g.z)
          bp.setXYZ(nb * 4 + 2, g.x - w / 2, y + 0.4, g.z)
          bp.setXYZ(nb * 4 + 3, g.x - w / 2 + w * f, y + 0.4, g.z)
          col.set(hpColor(f))
          for (let v = 0; v < 4; v++) bc.setXYZ(nb * 4 + v, col.r, col.g, col.b)
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
      bc.needsUpdate = true
      const sig = per.join(',') + '/' + np
      if (sig !== lastShown) {
        lastShown = sig
        setCounts(per)
        setPlayers(np)
      }
      if (selPos) {
        interest.visible = marker.visible = true
        interest.position.set(selPos.x, selY + 0.5, selPos.z)
        interest.scale.setScalar(pr.interest)
        marker.position.set(selPos.x, selY + 18, selPos.z)
        marker.rotation.y = now / 600
        if (pr.follow) controls.target.lerp(new THREE.Vector3(selPos.x, selY, selPos.z), 0.15)
      } else interest.visible = marker.visible = false
      controls.update()
      renderer.render(scene, camera)

      // The selected entity's tag follows it on screen.
      const el = tag.current
      if (el) {
        if (selPos && pr.selected !== null) {
          vec.set(selPos.x, selY + 24, selPos.z).project(camera)
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

  const legend = p.archetypes.map((name, i) => ({ name, i, n: counts[i] ?? 0, shape: SHAPES[i % SHAPES.length]!, color: p.colors?.[i] }))
  // Overlay text sits on ink bands so it reads over sky and grass.
  const band = 'bg-[#1a1a12]/75 text-[#f3ead2] font-mono text-[11px]'
  return (
    <div ref={box} className="relative min-h-[240px] h-full overflow-hidden" style={{ background: MAP.sky }}>
      <div
        ref={tag}
        aria-hidden="true"
        style={{ display: 'none' }}
        className={`absolute z-10 left-0 top-0 pointer-events-none whitespace-nowrap border border-[#ffd27a] px-1 leading-4 ${band}`}
      />
      {!failed && (
        <ul aria-label="Legend" className={`absolute z-10 left-3 bottom-8 m-0 px-2 py-1 list-none leading-5 pointer-events-none ${band}`}>
          {legend.map((l) => (
            <li key={l.name} className="flex items-center gap-2">
              <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true" fill={l.color ?? 'none'} stroke="currentColor" strokeWidth="1.5">
                <path d={l.shape.svg} />
              </svg>
              <span>{l.name}</span>
              <span className="opacity-70">{l.n}</span>
            </li>
          ))}
          <li className="flex items-center gap-2">
            <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true" fill={MAP.parchment} stroke="currentColor" strokeWidth="1.5">
              <path d="M8 1 L13 14 H3 Z" />
            </svg>
            <span>players</span>
            <span className="opacity-70">{players}</span>
          </li>
        </ul>
      )}
      <p className={`absolute z-10 left-3 bottom-2 m-0 px-2 tracking-wider pointer-events-none ${band}`} role="status">
        {failed ? 'WebGL is not available in this browser. The 2D map still works.' : 'drag to orbit · scroll to zoom · click to select'}
      </p>
    </div>
  )
}
