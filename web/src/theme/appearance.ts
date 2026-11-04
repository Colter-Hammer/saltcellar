// Palette and mode on <html> (docs/spec/visual-system.md#palettes-and-modes). The palette comes
// from the profile; the mode (Light / Dark / System) is per device and per login, in localStorage.
// The last of both is cached for index.html's inline script, which applies them before first paint.
import { effectScope, ref, watch } from 'vue'
import { useProfile } from '@/api/profile'
import { paletteOrDefault } from './palettes'

export type Mode = 'light' | 'dark' | 'system'

export const MODES: readonly { value: Mode; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'system', label: 'System' },
]

/** What index.html's inline script reads: the palette and mode last applied on this device. */
export const CACHED_PALETTE = 'saltcellar.palette'
export const CACHED_MODE = 'saltcellar.mode'

/** Where a login's mode lives on this device *(agent: by username, which the session names)*. */
function modeKey(username: string): string {
  return `saltcellar.mode.${username}`
}

function asMode(value: string | null): Mode {
  return value === 'light' || value === 'dark' ? value : 'system'
}

function read(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value)
  } catch {
    // Private browsing can refuse storage: the choice then lasts until reload.
  }
}

const DARK_QUERY = '(prefers-color-scheme: dark)'

function systemIsDark(): boolean {
  return globalThis.matchMedia(DARK_QUERY).matches
}

/** Puts `palette` (or the default) and `mode` on <html>, and caches both for the next load. */
function applyAppearance(palette: string | null | undefined, mode: Mode): void {
  const id = paletteOrDefault(palette)
  const dark = mode === 'dark' || (mode === 'system' && systemIsDark())
  const html = document.documentElement
  html.dataset.palette = id
  html.classList.toggle('dark', dark)
  html.style.colorScheme = dark ? 'dark' : 'light'
  write(CACHED_PALETTE, id)
  write(CACHED_MODE, mode)
}

/** The palette and mode on screen, shared by every caller. */
const palette = ref<string | null>(read(CACHED_PALETTE))
const mode = ref<Mode>(asMode(read(CACHED_MODE)))
let username: string | undefined
let started = false

/**
 * Keeps <html> in step with the logged-in login's palette and mode, System following the OS while
 * the app is open. Returns the mode and a setter for Settings › Profile.
 */
export function useAppearance() {
  const profile = useProfile()
  if (!started) {
    started = true
    // Detached, so it outlives the component that first asks (the shell goes on log out).
    effectScope(true).run(() => {
      watch([palette, mode], () => {
        applyAppearance(palette.value, mode.value)
      })
    })
    globalThis.matchMedia(DARK_QUERY).addEventListener('change', () => {
      if (mode.value === 'system') applyAppearance(palette.value, mode.value)
    })
  }
  watch(
    () => profile.data.value,
    (loaded) => {
      if (loaded === undefined) return
      if (loaded.username !== username) {
        username = loaded.username
        mode.value = asMode(read(modeKey(username)))
      }
      palette.value = loaded.palette ?? null
    },
    { immediate: true },
  )
  return {
    mode,
    setMode(next: Mode) {
      mode.value = next
      if (username !== undefined) write(modeKey(username), next)
    },
  }
}
