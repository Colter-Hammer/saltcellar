// What the visual system asks of every screen (docs/spec/visual-system.md), read off the page.
import type { Page } from '@playwright/test'

/** The tallest a phone's touch target may fall short of: 44px. */
const TOUCH = 44

/**
 * Every visible tappable thing on the page shorter than a touch target, described, so a failing
 * test names it. Inline links in running text take their line's height and aren't counted.
 */
export function smallTargets(page: Page): Promise<string[]> {
  return page.evaluate((touch) => {
    const tappable = document.querySelectorAll<HTMLElement>(
      'a, button, select, textarea, input:not([type="checkbox"], [type="radio"], [type="hidden"]), [role="option"], [role="menuitem"]',
    )
    return [...tappable]
      .filter((element) => {
        const box = element.getBoundingClientRect()
        if (box.width === 0 || box.height === 0) return false
        if (getComputedStyle(element).display === 'inline') return false
        return box.height < touch - 0.5
      })
      .map((element) => {
        const name = element.textContent.trim() || (element.getAttribute('aria-label') ?? '')
        const height = Math.round(element.getBoundingClientRect().height)
        return `${element.tagName.toLowerCase()} "${name}" ${String(height)}px`
      })
  }, TOUCH)
}
