import { useEffect, useState } from 'react'
import { api, type ChatLog } from '../api'
import { Empty, TextField, Toggle } from './Fields'

/** Live chat log from the server, plus a way to make any entity speak (handy for testing say rules). */
export function ChatPanel({ selected, active }: { selected: number | null; active: boolean }) {
  const [log, setLog] = useState<ChatLog | null>(null)
  const [text, setText] = useState('')
  const [shout, setShout] = useState(false)
  const [msg, setMsg] = useState<string | null>(null)

  useEffect(() => {
    if (!active) return
    let stop = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const tick = async () => {
      try {
        const r = await api<ChatLog>('/api/chat')
        if (!stop && r.status === 200) setLog(r.body)
      } catch {
        /* the status bar shows connectivity */
      }
      if (!stop) timer = setTimeout(tick, 1000)
    }
    void tick()
    return () => {
      stop = true
      if (timer) clearTimeout(timer)
    }
  }, [active])

  const speak = async () => {
    if (selected === null) return
    const r = await api<{ error?: string }>('/api/say', 'POST', { id: selected, text, shout })
    setMsg(r.status === 200 ? 'Sent.' : (r.body.error ?? `server said ${r.status}`))
    if (r.status === 200) setText('')
  }
  const lines = [...(log?.lines ?? [])].reverse()
  return (
    <div>
      <section aria-label="Speak as the selected entity" className="cell py-2">
        <h2 className="lbl pb-1">Speak as the selected entity</h2>
        {selected === null ? (
          <p className="text-fg-3">Select an entity on the map first.</p>
        ) : (
          <form
            className="flex items-end gap-2"
            onSubmit={(e) => {
              e.preventDefault()
              void speak()
            }}
          >
            <TextField label={`Entity ${selected & 0xffffff} says`} value={text} onCommit={setText} className="flex-1" />
            <Toggle label="Shout" checked={shout} onChange={setShout} />
            <button type="submit" className="btn btn-primary" disabled={text.trim() === ''}>
              Send
            </button>
          </form>
        )}
        <p aria-live="polite" className="min-h-6 text-fg-2">
          {msg}
        </p>
      </section>
      <section aria-label="Chat log" className="py-2">
        <h2 className="lbl pb-1">Recent chat, newest first</h2>
        {lines.length === 0 ? (
          <Empty>Nothing said yet. Players and bots talk through the same channel; say is heard within 64 units, shout by everyone.</Empty>
        ) : (
          <ol className="list-none m-0 p-0">
            {lines.map(([tick, from, ch, t, name], i) => (
              <li key={`${tick}-${from}-${i}`} className="cell flex gap-2">
                <span className="text-fg-3 w-14 shrink-0 tabular-nums">t{tick}</span>
                <span className="text-fg-2 w-28 shrink-0 truncate" title={`#${from & 0xffffff}`}>
                  {name || `#${from & 0xffffff}`}
                </span>
                <span className="lbl w-12 shrink-0 leading-6">{ch === 3 ? 'banner' : ch === 1 ? 'shout' : ch === 7 ? 'nearby' : 'say'}</span>
                <span className="min-w-0 break-words">{t}</span>
              </li>
            ))}
          </ol>
        )}
      </section>
    </div>
  )
}
