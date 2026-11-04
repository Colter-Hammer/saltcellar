// The palette registry (docs/spec/visual-system.md#theme-mechanism). Adding a palette is one CSS
// file in styles/palettes/ and one entry here. The swatches in Settings › Profile are drawn from
// the palette's own CSS, so no color value lives here *(agent)*.
import '@/styles/palettes/slate-teal.css'
import '@/styles/palettes/sage.css'
import '@/styles/palettes/deep-blue.css'
import '@/styles/palettes/plum.css'

export interface Palette {
  /** What `data-palette` and the profile carry. */
  id: string
  label: string
  /** What the swatch says under the name *(agent)*. */
  swatch: string
}

export const PALETTES: readonly Palette[] = [
  { id: 'slate-teal', label: 'Slate / teal', swatch: 'Teal on cool grays' },
  { id: 'sage', label: 'Sage', swatch: 'Olive on soft greens' },
  { id: 'deep-blue', label: 'Deep blue', swatch: 'Blue on cool blues' },
  { id: 'plum', label: 'Plum', swatch: 'Plum on warm mauves' },
]

export const DEFAULT_PALETTE = 'sage'

/** The palette to show for `id`: the default for none, or for one this app doesn't know. */
export function paletteOrDefault(id: string | null | undefined): string {
  return id != null && PALETTES.some((palette) => palette.id === id) ? id : DEFAULT_PALETTE
}
