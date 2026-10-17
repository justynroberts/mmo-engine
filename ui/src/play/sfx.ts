// A few synthesised blips. Off until the player turns them on (browsers want a gesture before audio anyway).

export type Sound = 'act' | 'hurt' | 'chat' | 'down'

const KEY = 'play-sound'
export const soundWanted = () => {
  try {
    return localStorage.getItem(KEY) === 'on'
  } catch {
    return false
  }
}
export const rememberSound = (on: boolean) => {
  try {
    localStorage.setItem(KEY, on ? 'on' : 'off')
  } catch {
    /* it just will not persist */
  }
}

let ctx: AudioContext | null = null

export function play(s: Sound) {
  try {
    ctx ??= new (window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext)()
    if (ctx.state === 'suspended') void ctx.resume()
    const t = ctx.currentTime
    const o = ctx.createOscillator()
    const g = ctx.createGain()
    const spec = {
      act: { f0: 880, f1: 220, len: 0.12, type: 'square' as const, vol: 0.05 },
      hurt: { f0: 180, f1: 60, len: 0.2, type: 'sawtooth' as const, vol: 0.07 },
      chat: { f0: 660, f1: 990, len: 0.06, type: 'sine' as const, vol: 0.04 },
      down: { f0: 300, f1: 40, len: 0.6, type: 'sawtooth' as const, vol: 0.08 },
    }[s]
    o.type = spec.type
    o.frequency.setValueAtTime(spec.f0, t)
    o.frequency.exponentialRampToValueAtTime(spec.f1, t + spec.len)
    g.gain.setValueAtTime(spec.vol, t)
    g.gain.exponentialRampToValueAtTime(0.0001, t + spec.len)
    o.connect(g).connect(ctx.destination)
    o.start(t)
    o.stop(t + spec.len + 0.02)
  } catch {
    /* no audio here; nothing to do */
  }
}
