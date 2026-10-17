import { useEffect, useRef, type MutableRefObject } from 'react'
import * as THREE from 'three'
import type { GameClient, PlayInfo } from './client'

export interface Target {
  id: number
  dist: number
}

interface Props {
  client: GameClient
  info: PlayInfo
  /** Written every frame: the nearest other entity, which is what an action will hit. */
  target: MutableRefObject<Target | null>
}

const GREEN = 0x00ff41
const DIM = 0x0a5a1e
const HOT = 0xd8ffe0
const CAP = 2048

/** A square grid of thin strips on the ground, centred on the origin, brighter on the centre lines. */
function thinGrid(size: number, step: number, w: number): THREE.Mesh {
  const n = Math.round(size / step)
  const half = size / 2
  const pos: number[] = []
  const col: number[] = []
  const strip = (x0: number, z0: number, x1: number, z1: number, c: THREE.Color) => {
    pos.push(x0, 0, z0, x1, 0, z0, x1, 0, z1, x0, 0, z0, x1, 0, z1, x0, 0, z1)
    for (let i = 0; i < 6; i++) col.push(c.r, c.g, c.b)
  }
  const bright = new THREE.Color(GREEN)
  const dim = new THREE.Color(DIM)
  for (let i = 0; i <= n; i++) {
    const k = -half + i * step
    const c = i === n / 2 ? bright : dim
    strip(-half, k - w / 2, half, k + w / 2, c) // along x
    strip(k - w / 2, -half, k + w / 2, half, c) // along z
  }
  const g = new THREE.BufferGeometry()
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3))
  g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3))
  return new THREE.Mesh(g, new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.DoubleSide }))
}

interface Ghost {
  x: number
  z: number
  tx: number
  tz: number
  hp: number
}

/** The world around you, as a green wireframe, from just above and behind. */
export default function PlayScene({ client, info, target }: Props) {
  const box = useRef<HTMLDivElement>(null)
  const ref = useRef({ client, info, target })
  ref.current = { client, info, target }

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
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2))
    renderer.setClearColor(0x000000, 1)
    const cv = renderer.domElement
    cv.className = 'absolute inset-0 h-full w-full'
    cv.setAttribute('role', 'img')
    cv.setAttribute('aria-label', 'Wireframe view of the world around your character. Move with W A S D or the arrow keys, act with space.')
    host.appendChild(cv)

    const half = info.world_size / 2
    const scene = new THREE.Scene()
    scene.fog = new THREE.Fog(0x000000, 50, 150)
    const camera = new THREE.PerspectiveCamera(55, 1, 0.5, 900)

    // A patch of grid that follows the player, snapped so it never appears to slide. Drawn as thin quads rather
    // than GL lines: line rasterisation is the least consistent part of WebGL (software renderers drop steep
    // lines), while triangles look the same everywhere.
    const STEP = 8
    const grid = thinGrid(240, STEP, 0.07)
    scene.add(grid)
    const wallGeoms: THREE.BufferGeometry[] = []
    const wallMat = new THREE.LineBasicMaterial({ color: GREEN })
    // The edge of the world.
    const edge = new THREE.EdgesGeometry(new THREE.BoxGeometry(info.world_size, 4, info.world_size))
    edge.translate(0, 2, 0)
    scene.add(new THREE.LineSegments(edge, wallMat))
    wallGeoms.push(edge)
    for (const [x0, y0, x1, y1] of info.boxes) {
      const g = new THREE.EdgesGeometry(new THREE.BoxGeometry(x1 - x0, 10, y1 - y0))
      g.translate((x0 + x1) / 2 - half, 5, (y0 + y1) / 2 - half)
      wallGeoms.push(g)
      scene.add(new THREE.LineSegments(g, wallMat))
    }

    const wire = (c: number) => new THREE.MeshBasicMaterial({ color: c, wireframe: true })
    const others = new THREE.InstancedMesh(new THREE.OctahedronGeometry(0.9), wire(GREEN), CAP)
    const me = new THREE.Mesh(new THREE.ConeGeometry(0.9, 2.6, 4), wire(HOT))
    const ring = new THREE.Mesh(new THREE.RingGeometry(1.4, 1.55, 32).rotateX(-Math.PI / 2), new THREE.MeshBasicMaterial({ color: HOT, side: THREE.DoubleSide }))
    ring.visible = false
    others.frustumCulled = false
    others.count = 0
    scene.add(others, me, ring)

    // Health bars: one line segment pair per hurt entity, rebuilt every frame in a fixed buffer.
    const bars = new THREE.LineSegments(
      new THREE.BufferGeometry().setAttribute('position', new THREE.BufferAttribute(new Float32Array(CAP * 12), 3)),
      new THREE.LineBasicMaterial({ color: HOT }),
    )
    bars.frustumCulled = false
    scene.add(bars)

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
    const tmp = new THREE.Object3D()
    const cam = new THREE.Vector3()
    let camReady = false
    let last = performance.now()
    let raf = 0
    const rangeOf = () => ref.current.info.actions[0]?.range ?? 8
    const frame = (now: number) => {
      raf = requestAnimationFrame(frame)
      const dt = Math.min(0.1, (now - last) / 1000)
      last = now
      const { client: c, info: inf, target: tg } = ref.current
      const ents = c.entities()
      const seen = new Set<number>()
      const hpMax = inf.attrs.find((a) => a.name === 'health')?.max ?? 100
      for (const e of ents) {
        seen.add(e.id)
        const g = ghosts.get(e.id)
        const hp = e.attrs.health !== undefined ? e.attrs.health / hpMax : 1
        if (g) {
          g.tx = e.x - half
          g.tz = e.y - half
          g.hp = hp
        } else ghosts.set(e.id, { x: e.x - half, z: e.y - half, tx: e.x - half, tz: e.y - half, hp })
      }
      for (const id of ghosts.keys()) if (!seen.has(id)) ghosts.delete(id)

      // The server sends 20 snapshots a second; glide so motion looks continuous.
      const k = 1 - Math.exp(-dt * 14)
      let n = 0
      let nb = 0
      const bp = bars.geometry.getAttribute('position') as THREE.BufferAttribute
      const self = ghosts.get(c.entity)
      let best: Target | null = null
      let bestG: Ghost | null = null
      for (const [id, g] of ghosts) {
        g.x += (g.tx - g.x) * k
        g.z += (g.tz - g.z) * k
        if (id === c.entity) continue
        if (n < CAP) {
          tmp.position.set(g.x, 1, g.z)
          tmp.rotation.set(0, now / 1500 + id, 0)
          tmp.updateMatrix()
          others.setMatrixAt(n++, tmp.matrix)
        }
        if (g.hp < 0.995 && nb < CAP) {
          const w = 1.6
          const y = 2.6
          bp.setXYZ(nb * 4, g.x - w / 2, y, g.z)
          bp.setXYZ(nb * 4 + 1, g.x - w / 2 + w * Math.max(0, g.hp), y, g.z)
          bp.setXYZ(nb * 4 + 2, g.x - w / 2, y + 0.01, g.z)
          bp.setXYZ(nb * 4 + 3, g.x - w / 2 + w * Math.max(0, g.hp), y + 0.01, g.z)
          nb++
        }
        if (self) {
          const d = Math.hypot(g.tx - self.tx, g.tz - self.tz)
          if (!best || d < best.dist) {
            best = { id, dist: d }
            bestG = g
          }
        }
      }
      others.count = n
      others.instanceMatrix.needsUpdate = true
      bars.geometry.setDrawRange(0, nb * 4)
      bp.needsUpdate = true
      tg.current = best

      if (self) {
        me.visible = true
        me.position.set(self.x, 1.3, self.z)
        me.rotation.y = now / 1200
        if (!camReady) {
          cam.set(self.x, 34, self.z + 30)
          camReady = true
        }
        cam.x += (self.x - cam.x) * (1 - Math.exp(-dt * 6))
        cam.z += (self.z + 30 - cam.z) * (1 - Math.exp(-dt * 6))
        grid.position.set(Math.round(self.x / STEP) * STEP, 0, Math.round(self.z / STEP) * STEP)
        camera.position.set(cam.x, 34, cam.z)
        camera.lookAt(self.x, 0, self.z)
        ring.visible = !!bestG && best!.dist <= rangeOf()
        if (bestG) ring.position.set(bestG.x, 0.2, bestG.z)
      } else {
        me.visible = false
        ring.visible = false
      }
      renderer.render(scene, camera)
    }
    raf = requestAnimationFrame(frame)

    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
      for (const g of wallGeoms) g.dispose()
      wallMat.dispose()
      grid.geometry.dispose()
      ;(grid.material as THREE.Material).dispose()
      for (const o of [others, me, ring, bars]) {
        o.geometry.dispose()
        ;(o.material as THREE.Material).dispose()
      }
      renderer.dispose()
      cv.remove()
    }
  }, [info])

  return <div ref={box} className="absolute inset-0 bg-black overflow-hidden text-[#00ff41] p-4" />
}
