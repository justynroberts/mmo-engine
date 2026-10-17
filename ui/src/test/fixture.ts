import type { GameFile } from '../api'

export const fixture = (): GameFile => ({
  move_speed: 20,
  entity_radius: 0.5,
  interest_radius: 128,
  max_visible: 64,
  attrs: [
    { name: 'health', min: 0, max: 100, default: 100, bits: 8, replicated: true, regen_per_sec: 0, on_zero: 'respawn' },
    { name: 'energy', min: 0, max: 100, default: 100, bits: 8, replicated: false, regen_per_sec: 5, on_zero: 'none' },
  ],
  actions: [
    {
      id: 1,
      name: 'zap',
      cooldown_ticks: 10,
      range: 8,
      requires: { attr: 'energy', min: 5 },
      effects: [
        { to: 'target', attr: 'health', delta: -10 },
        { to: 'self', attr: 'energy', delta: -5 },
      ],
    },
  ],
  archetypes: [
    { name: 'drone', color: '#e0457b', count: 4, behaviour: 'chase', speed_mult: 1, sight: 40, attack: 'zap', attrs: { health: 60 }, home: null, rules: [] },
  ],
  places: [],
  routes: [],
})
