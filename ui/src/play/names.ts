// Display names (crates/proto/src/names.rs): the web gateway's /play/names says what each entity is called, a
// player's chosen name or a bot's type. Ids we do not know yet show as their number until the answer comes back.

import { playUrl } from './base'

/** Most ids one request may ask about (web.rs NAMES_MAX). */
export const NAMES_MAX = 256

export class NameBook {
  readonly known = new Map<number, string>()
  private want = new Set<number>()
  private busy = false

  /** What to call an entity now: its name if known, else its number (and it is asked about next poll). */
  name(id: number): string {
    const n = this.known.get(id)
    if (n !== undefined) return n
    this.want.add(id)
    return `#${id & 0xffffff}`
  }

  /** Ask again about these, e.g. everyone in view now and then, since players can rename. */
  refresh(ids: Iterable<number>) {
    for (const id of ids) this.want.add(id)
  }

  /** Fetch what is wanted. Resolves with the ids that got a new name (empty if nothing changed). */
  async poll(fetchJson: (url: string) => Promise<unknown> = (u) => fetch(u).then((r) => (r.ok ? r.json() : null))): Promise<number[]> {
    if (this.busy || this.want.size === 0) return []
    const ids = [...this.want].slice(0, NAMES_MAX)
    ids.forEach((id) => this.want.delete(id))
    this.busy = true
    try {
      const got = (await fetchJson(playUrl(`/play/names?ids=${ids.join(',')}`))) as Record<string, string> | null
      const changed: number[] = []
      for (const [k, v] of Object.entries(got ?? {})) {
        const id = Number(k)
        // A bot with a name of its own comes as "Kind|Given" (the Godot client matches shops by kind); shown here
        // as "Given the Kind".
        const shown = typeof v === 'string' && v.includes('|') ? `${v.split('|')[1]} the ${v.split('|')[0]}` : v
        if (typeof shown === 'string' && this.known.get(id) !== shown) {
          this.known.set(id, shown)
          changed.push(id)
        }
      }
      return changed
    } catch {
      ids.forEach((id) => this.want.add(id)) // try again next time
      return []
    } finally {
      this.busy = false
    }
  }
}
