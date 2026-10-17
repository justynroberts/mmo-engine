import { useRef, useState } from 'react'

const R = 48

/** An analogue thumb stick for touch screens. Reports a vector with each axis in -1..1, and 0,0 on release. */
export default function Joystick({ onMove }: { onMove: (x: number, y: number) => void }) {
  const base = useRef<HTMLDivElement>(null)
  const [knob, setKnob] = useState({ x: 0, y: 0 })
  const active = useRef<number | null>(null)

  const update = (e: React.PointerEvent) => {
    const r = base.current?.getBoundingClientRect()
    if (!r) return
    let dx = e.clientX - (r.left + r.width / 2)
    let dy = e.clientY - (r.top + r.height / 2)
    const len = Math.hypot(dx, dy)
    if (len > R) {
      dx = (dx / len) * R
      dy = (dy / len) * R
    }
    setKnob({ x: dx, y: dy })
    // A small dead zone so a resting thumb does not drift the character.
    const m = Math.hypot(dx, dy) / R
    onMove(m < 0.2 ? 0 : dx / R, m < 0.2 ? 0 : dy / R)
  }
  const end = () => {
    active.current = null
    setKnob({ x: 0, y: 0 })
    onMove(0, 0)
  }

  return (
    <div
      ref={base}
      role="presentation"
      data-testid="joystick"
      className="relative rounded-full touch-none select-none"
      style={{ width: R * 2 + 16, height: R * 2 + 16, background: 'var(--hud-panel)', border: '1px solid var(--hud-edge)' }}
      onPointerDown={(e) => {
        active.current = e.pointerId
        e.currentTarget.setPointerCapture?.(e.pointerId)
        update(e)
      }}
      onPointerMove={(e) => active.current === e.pointerId && update(e)}
      onPointerUp={end}
      onPointerCancel={end}
    >
      <div
        className="absolute rounded-full"
        style={{
          width: 36,
          height: 36,
          left: '50%',
          top: '50%',
          marginLeft: -18,
          marginTop: -18,
          transform: `translate(${knob.x}px, ${knob.y}px)`,
          background: 'var(--hud-hot)',
          opacity: 0.85,
        }}
      />
    </div>
  )
}
