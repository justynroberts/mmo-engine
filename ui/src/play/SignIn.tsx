import { useState } from 'react'
import type { PlayInfo } from './client'
import { identityFromPassphrase, parseRecoveryCode } from './protocol'

type Mode = 'account' | 'recovery' | 'guest'

const remembered = () => {
  try {
    return localStorage.getItem('play-account') ?? ''
  } catch {
    return ''
  }
}

interface Props {
  info: PlayInfo | null
  infoError: string | null
  /** Why the server turned us away, if it did. */
  rejected: string | null
  onEnter: (identity: Uint8Array | null) => void
}

export default function SignIn({ info, infoError, rejected, onEnter }: Props) {
  const [mode, setMode] = useState<Mode>('account')
  const [account, setAccount] = useState(remembered)
  const [pass, setPass] = useState('')
  const [code, setCode] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const begin = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!info) return
    setError(null)
    let id: Uint8Array | null = null
    if (mode === 'account') {
      if (!account.trim() || pass.length < 8) {
        setError('Pick an account name and a passphrase of at least 8 characters. Longer is better: it is the only thing protecting the character.')
        return
      }
      setBusy(true)
      id = await identityFromPassphrase(account, pass)
      try {
        localStorage.setItem('play-account', account.trim())
      } catch {
        /* fine */
      }
      setPass('')
    } else if (mode === 'recovery') {
      id = parseRecoveryCode(code)
      if (!id) {
        setError('That recovery code does not check out. Look for a mistyped character.')
        return
      }
      setBusy(true)
    }
    onEnter(id)
    setBusy(false)
  }

  return (
    <main className="h-full overflow-y-auto">
      <div className="max-w-[34rem] px-6 pt-16 pb-12">
        <h1 className="text-head m-0 font-medium tracking-wide">
          mmo<span className="text-accent">/</span>engine
        </h1>
        <p className="text-fg-3 mt-0">
          {infoError ? `Cannot reach the game: ${infoError}` : info ? 'Sign in to keep your character, or just jack in.' : 'Loading the world…'}
        </p>
        {rejected && (
          <div role="alert" className="text-accent pb-2">
            <p className="m-0">{rejected}</p>
            <button type="button" className="btn mt-1" onClick={() => location.reload()}>
              Reload the page
            </button>
          </div>
        )}
        <div role="tablist" aria-label="How to sign in" className="flex gap-4 border-b border-line">
          {(
            [
              ['account', 'Account'],
              ['recovery', 'Recovery code'],
              ['guest', 'Guest'],
            ] as const
          ).map(([m, l]) => (
            <button
              key={m}
              role="tab"
              type="button"
              aria-selected={mode === m}
              onClick={() => setMode(m)}
              className={`tab ${mode === m ? 'text-fg border-b-accent' : ''}`}
              data-state={mode === m ? 'active' : 'inactive'}
            >
              {l}
            </button>
          ))}
        </div>
        <form onSubmit={begin} className="pt-4 grid gap-3">
          {mode === 'account' && (
            <>
              <label className="grid gap-1">
                <span className="lbl">Account name</span>
                <input className="field" autoComplete="username" value={account} onChange={(e) => setAccount(e.target.value)} />
              </label>
              <label className="grid gap-1">
                <span className="lbl">Passphrase</span>
                <input className="field" type="password" autoComplete="current-password" value={pass} onChange={(e) => setPass(e.target.value)} />
              </label>
              <p className="text-label text-fg-3 m-0">
                Your passphrase never leaves this page. It is stretched here into the key that proves who you are, so the same name and passphrase bring the same
                character back from any device. There is no reset: if you lose it, use a recovery code (shown once you are in).
              </p>
            </>
          )}
          {mode === 'recovery' && (
            <label className="grid gap-1">
              <span className="lbl">Recovery code</span>
              <input className="field" spellCheck={false} autoComplete="off" value={code} onChange={(e) => setCode(e.target.value)} placeholder="xxxxxxxx-xxxxxxxx-…" />
            </label>
          )}
          {mode === 'guest' && <p className="m-0 text-fg-3">Nothing is saved for a guest: when you leave, the character is gone.</p>}
          {error && (
            <p role="alert" className="m-0 text-accent">
              {error}
            </p>
          )}
          <div>
            <button className="btn btn-primary" type="submit" disabled={!info || busy}>
              {busy ? 'Working…' : 'Enter the world'}
            </button>
          </div>
        </form>
      </div>
    </main>
  )
}
