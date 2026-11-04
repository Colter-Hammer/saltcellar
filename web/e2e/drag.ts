// Dragging a sortable list's grip (src/lib/sortable.ts) the way a person does: down on the grip,
// a run of moves to just inside the target, up. A mouse for a desktop, real touch events (through
// the DevTools protocol) for a phone, which is what a finger on the grip sends.
import type { Locator, Page } from '@playwright/test'

interface Options {
  /** Where in the target to let go: its top quarter, or its bottom quarter (the default). */
  to?: 'top' | 'bottom'
  /** Drive touch events, not the mouse. */
  touch?: boolean
  /** Runs with the pointer still down, the row lifted and the others slid. */
  during?: () => Promise<void>
}

const STEPS = 16

async function centre(locator: Locator) {
  const box = await locator.boundingBox()
  if (box === null) throw new Error('nothing to drag: it is not on screen')
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 }
}

/** Drags `grip` to the `target` row. */
export async function drag(page: Page, grip: Locator, target: Locator, options: Options = {}) {
  // A drag can't scroll to its target, so both ends are brought on screen first (the grip last).
  await target.scrollIntoViewIfNeeded()
  await grip.scrollIntoViewIfNeeded()
  const from = await centre(grip)
  const box = await target.boundingBox()
  if (box === null) throw new Error('nowhere to drop: the target is not on screen')
  const y = options.to === 'top' ? box.y + box.height * 0.25 : box.y + box.height * 0.75
  const path = Array.from({ length: STEPS }, (_, i) => ({
    x: from.x,
    y: from.y + ((y - from.y) * (i + 1)) / STEPS,
  }))

  if (options.touch) {
    const touch = await page.context().newCDPSession(page)
    const send = (
      type: 'touchStart' | 'touchMove' | 'touchEnd',
      point?: { x: number; y: number },
    ) =>
      touch.send('Input.dispatchTouchEvent', {
        type,
        touchPoints: point ? [{ x: point.x, y: point.y }] : [],
      })
    await send('touchStart', from)
    for (const point of path) {
      await send('touchMove', point)
      await page.waitForTimeout(20)
    }
    await options.during?.()
    await send('touchEnd')
    await touch.detach()
    return
  }

  await page.mouse.move(from.x, from.y)
  await page.mouse.down()
  for (const point of path) {
    await page.mouse.move(point.x, point.y)
    await page.waitForTimeout(20)
  }
  await options.during?.()
  await page.mouse.up()
}
