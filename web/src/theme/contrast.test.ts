// The contrast check (docs/spec/visual-system.md#contrast): every palette file, light and dark,
// passes WCAG AA on the spec's pairs, and its category colors stay apart. A failure names the
// palette, mode, pair and ratio.
import { describe, expect, it } from 'vitest'
import { PALETTES } from './palettes'

const files = import.meta.glob<string>('../styles/palettes/*.css', {
  query: '?raw',
  import: 'default',
  eager: true,
})

const CATS = Array.from({ length: 12 }, (_, i) => `--cat-${String(i + 1)}`)

/** Every token a palette file sets, in each mode. */
const TOKENS = [
  '--background',
  '--card',
  '--muted',
  '--border',
  '--foreground',
  '--muted-foreground',
  '--primary',
  '--primary-foreground',
  ...CATS,
  '--money-in',
  '--over-budget',
  '--near-limit',
  '--chart-out',
]

/** [text, surface, minimum ratio] */
const PAIRS: [string, string, number][] = [
  ...['--foreground', '--muted-foreground'].flatMap((text) =>
    ['--background', '--card', '--muted'].map((bg): [string, string, number] => [text, bg, 4.5]),
  ),
  ...['--primary', '--money-in', '--over-budget', '--near-limit'].flatMap((text) =>
    ['--background', '--card'].map((bg): [string, string, number] => [text, bg, 4.5]),
  ),
  ['--primary-foreground', '--primary', 4.5],
  ...['--chart-out', ...CATS].map((mark): [string, string, number] => [mark, '--card', 3]),
]

/** The closest two `--cat-n` in a palette and mode, in OKLab. */
const MIN_CAT_DISTANCE = 0.09

type Rgb = [number, number, number]

function idOf(path: string): string {
  return path.replace(/^.*\/|\.css$/g, '')
}

/** The declarations in the rule whose selector is exactly `selector`; null when there is none. */
function block(css: string, selector: string): Map<string, string> | null {
  for (const [, sel = '', body = ''] of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    if (sel.replace(/\/\*[\s\S]*?\*\//g, '').trim() !== selector) continue
    const tokens = new Map<string, string>()
    for (const [, name = '', value = ''] of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g))
      tokens.set(name, value.trim())
    return tokens
  }
  return null
}

function hex(value: string): Rgb | null {
  const match = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(value)
  if (!match) return null
  return [match[1], match[2], match[3]].map((part) => parseInt(part ?? '', 16) / 255) as Rgb
}

function linear(channel: number): number {
  return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
}

function luminance([r, g, b]: Rgb): number {
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

function ratio(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number]
  return (hi + 0.05) / (lo + 0.05)
}

/** sRGB to OKLab (https://bottosson.github.io/posts/oklab/). */
function oklab(rgb: Rgb): Rgb {
  const [r, g, b] = rgb.map(linear) as Rgb
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817104686 * g + 0.6299769004 * b)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ]
}

/** What's wrong with one palette in one mode: one line per problem, empty when it passes. */
function problems(id: string, mode: 'light' | 'dark', css: string): string[] {
  const where = `${id} ${mode}`
  const selector = mode === 'light' ? `[data-palette='${id}']` : `[data-palette='${id}'].dark`
  const tokens = block(css, selector)
  if (!tokens) return [`${where}: no ${selector} rule`]

  const colors = new Map<string, Rgb>()
  const found: string[] = []
  for (const token of TOKENS) {
    const value = tokens.get(token)
    const rgb = value === undefined ? null : hex(value)
    if (value === undefined) found.push(`${where}: ${token} is missing`)
    else if (!rgb) found.push(`${where}: ${token} is ${value}, not a #rrggbb hex`)
    else colors.set(token, rgb)
  }

  for (const [text, bg, min] of PAIRS) {
    const [a, b] = [colors.get(text), colors.get(bg)]
    if (!a || !b) continue
    const r = ratio(a, b)
    if (r < min)
      found.push(`${where}: ${text} on ${bg} is ${r.toFixed(2)}:1, needs ${String(min)}:1`)
  }

  let closest: [number, string, string] | null = null
  for (const [i, x] of CATS.entries())
    for (const y of CATS.slice(i + 1)) {
      const [a, b] = [colors.get(x), colors.get(y)]
      if (!a || !b) continue
      const [la, lb] = [oklab(a), oklab(b)]
      const distance = Math.hypot(la[0] - lb[0], la[1] - lb[1], la[2] - lb[2])
      if (!closest || distance < closest[0]) closest = [distance, x, y]
    }
  if (closest && closest[0] < MIN_CAT_DISTANCE)
    found.push(
      `${where}: ${closest[1]} and ${closest[2]} are ${closest[0].toFixed(3)} apart in OKLab, needs ${String(MIN_CAT_DISTANCE)}`,
    )
  return found
}

describe('the contrast check', () => {
  it('reads one file per registry entry, and no file without one', () => {
    const ids = Object.keys(files).map(idOf).sort()
    expect(ids).toEqual(PALETTES.map((palette) => palette.id).sort())
  })

  it.each(Object.entries(files).map(([path, css]) => [idOf(path), css]))(
    '%s passes WCAG AA in light and dark',
    (id, css) => {
      expect([...problems(id, 'light', css), ...problems(id, 'dark', css)]).toEqual([])
    },
  )
})

// On the shipped default, edited; each edit is the first match, in the light rule.
describe('problems', () => {
  const css = files['../styles/palettes/deep-blue.css'] ?? ''

  it('names the palette, mode, pair and ratio when muted text is too light', () => {
    const lighter = css.replace(/--muted-foreground: #\w+;/, '--muted-foreground: #9a9a9a;')
    expect(problems('deep-blue', 'light', lighter)).toContain(
      'deep-blue light: --muted-foreground on --card is 2.81:1, needs 4.5:1',
    )
  })

  it('fails a missing token', () => {
    const missing = css.replace(/--chart-out: #\w+;/, '')
    expect(problems('deep-blue', 'light', missing)).toContain(
      'deep-blue light: --chart-out is missing',
    )
  })

  it('fails a value that is not hex', () => {
    const named = css.replace(/--primary: #\w+;/, '--primary: navy;')
    expect(problems('deep-blue', 'light', named)).toContain(
      'deep-blue light: --primary is navy, not a #rrggbb hex',
    )
  })

  it('fails category colors that are too close', () => {
    const cat1 = /--cat-1: (#\w+);/.exec(css)?.[1] ?? ''
    const close = css.replace(/--cat-2: #\w+;/, `--cat-2: ${cat1};`)
    expect(problems('deep-blue', 'light', close)).toContain(
      'deep-blue light: --cat-1 and --cat-2 are 0.000 apart in OKLab, needs 0.09',
    )
  })

  it('fails a file without the mode it is asked for', () => {
    expect(problems('deep-blue', 'dark', css.replace('.dark', ''))).toEqual([
      "deep-blue dark: no [data-palette='deep-blue'].dark rule",
    ])
  })
})
