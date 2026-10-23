import * as Switch from '@radix-ui/react-switch'
import { useEffect, useId, useState, type ReactNode } from 'react'
import { parseNum } from '../gameEdit'

/** Label above a control, like a channel strip legend. */
export function LF({ label, children, className = '' }: { label: string; children: ReactNode; className?: string }) {
  return (
    <label className={`block min-w-0 ${className}`}>
      <span className="lbl block truncate">{label}</span>
      {children}
    </label>
  )
}

export function NumField({
  label,
  value,
  onCommit,
  step,
  className,
}: {
  label: string
  value: number
  onCommit: (n: number) => void
  step?: number
  className?: string
}) {
  const [text, setText] = useState(String(value))
  const [focus, setFocus] = useState(false)
  useEffect(() => {
    if (!focus) setText(String(value))
  }, [value, focus])
  const bad = parseNum(text) === null
  return (
    <LF label={label} className={className}>
      <input
        className="field num"
        inputMode="decimal"
        value={text}
        step={step}
        aria-invalid={bad}
        onFocus={() => setFocus(true)}
        onBlur={() => {
          setFocus(false)
          setText(String(value))
        }}
        onChange={(e) => {
          setText(e.target.value)
          const n = parseNum(e.target.value)
          if (n !== null) onCommit(n)
        }}
      />
    </LF>
  )
}

export function TextField({
  label,
  value,
  onCommit,
  className,
  invalid,
  allowEmpty,
  maxLength,
}: {
  label: string
  value: string
  onCommit: (s: string) => void
  className?: string
  invalid?: boolean
  /** Commit an empty value too (an optional field being cleared). */
  allowEmpty?: boolean
  maxLength?: number
}) {
  const [text, setText] = useState(value)
  const [focus, setFocus] = useState(false)
  useEffect(() => {
    if (!focus) setText(value)
  }, [value, focus])
  return (
    <LF label={label} className={className}>
      <input
        className="field"
        value={text}
        spellCheck={false}
        aria-invalid={invalid}
        maxLength={maxLength}
        onFocus={() => setFocus(true)}
        onBlur={() => {
          setFocus(false)
          setText(value)
        }}
        onChange={(e) => {
          setText(e.target.value)
          if (allowEmpty || e.target.value.trim() !== '') onCommit(e.target.value)
        }}
      />
    </LF>
  )
}

export function SelectField<T extends string>({
  label,
  value,
  options,
  onChange,
  className,
}: {
  label: string
  value: T
  options: { value: T; label: string }[]
  onChange: (v: T) => void
  className?: string
}) {
  return (
    <LF label={label} className={className}>
      <select className="field cursor-pointer" value={value} onChange={(e) => onChange(e.target.value as T)}>
        {options.map((o) => (
          <option key={o.value} value={o.value} className="bg-bg text-fg">
            {o.label}
          </option>
        ))}
      </select>
    </LF>
  )
}

export function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  const id = useId()
  return (
    <div className="min-w-0">
      <span className="lbl block truncate" id={id}>
        {label}
      </span>
      <Switch.Root
        aria-labelledby={id}
        checked={checked}
        onCheckedChange={onChange}
        className="relative block h-4 w-8 mt-1 rounded-sm border border-line bg-transparent cursor-pointer data-[state=checked]:border-accent"
      >
        <Switch.Thumb className="block h-2 w-2 ml-[3px] rounded-none bg-fg-3 transition-transform duration-150 ease-out data-[state=checked]:translate-x-4 data-[state=checked]:bg-accent" />
      </Switch.Root>
    </div>
  )
}

/** One row of the grid: a bordered strip with a title line and a body. */
export function Strip({ children }: { children: ReactNode }) {
  return <div className="cell py-2">{children}</div>
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="text-fg-3 py-4">{children}</p>
}
