/** @type {import('tailwindcss').Config} */
export default {
  darkMode: 'class',
  content: ['./index.html', './play.html', './src/**/*.{ts,tsx}'],
  theme: {
    fontSize: { label: ['11px', '16px'], base: ['13px', '24px'], head: ['18px', '24px'] },
    borderRadius: { none: '0', sm: 'var(--radius-sm)', DEFAULT: 'var(--radius-sm)' },
    extend: {
      colors: {
        bg: 'var(--color-bg)',
        'bg-2': 'var(--color-bg-2)',
        'bg-3': 'var(--color-bg-3)',
        line: 'var(--color-line)',
        fg: 'var(--color-fg)',
        'fg-2': 'var(--color-fg-2)',
        'fg-3': 'var(--color-fg-3)',
        accent: 'var(--color-accent)',
        'accent-ink': 'var(--color-accent-ink)',
      },
      fontFamily: { sans: ['"Instrument Sans Variable"', '"Instrument Sans"', 'system-ui', 'sans-serif'], mono: ['"Martian Mono"', 'ui-monospace', 'monospace'] },
    },
  },
  plugins: [],
}
