// Writes the fixed-color tile icons in public/ from src/assets/logo/mark.svg
// (docs/spec/visual-system.md#logo). Run with `just icons` after changing the mark;
// checks/icons.test.ts fails while public/favicon.svg is out of date.
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { chromium } from '@playwright/test'

/** The tile's two colors, the same whatever the palette. */
export const TILE = { background: '#1d3a40', mark: '#f4f1ea' }

/** Each PNG: its side, and how much of the side the 32-unit mark box fills. */
export const PNGS = [
  // iOS rounds the corners itself, so a square full-bleed tile *(agent)*.
  { file: 'apple-touch-icon.png', size: 180, mark: 0.6875, radius: 0 },
  { file: 'icon-192.png', size: 192, mark: 0.6875, radius: 7 },
  { file: 'icon-512.png', size: 512, mark: 0.6875, radius: 7 },
  // Full bleed, the mark well inside the central 80% circle that every mask shape keeps.
  { file: 'icon-maskable-512.png', size: 512, mark: 0.55, radius: 0 },
] as const

const markPath = join(import.meta.dirname, '../src/assets/logo/mark.svg')
const publicDir = join(import.meta.dirname, '../public')

/** What's inside the mark's `<svg>`: its shapes, without the wrapper. */
export function markBody(svg: string): string {
  const body = /<svg[^>]*>([\s\S]*)<\/svg>/.exec(svg)?.[1]
  if (body === undefined) throw new Error('mark.svg has no <svg> element')
  return body
    .trim()
    .split('\n')
    .map((line) => line.trim())
    .join('\n    ')
}

/** The mark on the tile, on a 32-unit square: `scale` of it for the mark, corners of `radius`. */
export function tileSvg(markSvg: string, { scale = 0.6875, radius = 7 } = {}): string {
  const offset = (32 * (1 - scale)) / 2
  return [
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">',
    `  <rect width="32" height="32" rx="${String(radius)}" fill="${TILE.background}"/>`,
    `  <g transform="translate(${String(offset)} ${String(offset)}) scale(${String(scale)})" color="${TILE.mark}" fill="currentColor">`,
    `    ${markBody(markSvg)}`,
    '  </g>',
    '</svg>',
    '',
  ].join('\n')
}

export function readMark(): string {
  return readFileSync(markPath, 'utf8')
}

async function main() {
  const mark = readMark()
  writeFileSync(join(publicDir, 'favicon.svg'), tileSvg(mark))
  const browser = await chromium.launch()
  try {
    for (const png of PNGS) {
      const page = await browser.newPage({ viewport: { width: png.size, height: png.size } })
      const svg = tileSvg(mark, { scale: png.mark, radius: png.radius })
      await page.setContent(
        `<style>html,body{margin:0;background:transparent}svg{display:block;width:100vw;height:100vh}</style>${svg}`,
      )
      await page.screenshot({ path: join(publicDir, png.file), omitBackground: true })
      await page.close()
    }
  } finally {
    await browser.close()
  }
}

if (import.meta.main) await main()
