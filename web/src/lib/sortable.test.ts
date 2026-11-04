// The sortable in jsdom, which has no layout: rows are 50px high and stand one under another in the
// order on screen (200px for row 4), read back through stubbed rects and `elementsFromPoint`, and
// animation frames run by hand, so each step of a drag is under the test's control.
import { effectScope, nextTick, ref, type EffectScope } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { type Over, useSortable } from './sortable'

const ROW = 50
const heightOf = (id: number) => (id === 4 ? 200 : ROW)

let scope: EffectScope
let order: ReturnType<typeof ref<number[]>>
let saved: number[][]
let overs: Over[]
let sortable: ReturnType<typeof useSortable<number[]>>
let frames: FrameRequestCallback[]
let reduced: boolean
let animate: ReturnType<typeof vi.fn>
let grips: Map<number, HTMLElement>
let rows: Map<number, HTMLElement>
let edge: HTMLElement
let scrollBy: ReturnType<typeof vi.fn>

/** Where row `id` is, by its place in the order on screen. */
function top(id: number) {
  return sortable.shown.value
    .slice(0, sortable.shown.value.indexOf(id))
    .reduce((sum, before) => sum + heightOf(before), 0)
}

function rect(id: number) {
  const y = top(id)
  const h = heightOf(id)
  return { x: 0, y, left: 0, top: y, right: 200, bottom: y + h, width: 200, height: h }
}

function pointer(type: string, init: Partial<PointerEventInit> = {}) {
  return new PointerEvent(type, {
    bubbles: true,
    cancelable: true,
    pointerId: 1,
    pointerType: 'mouse',
    button: 0,
    ...init,
  })
}

/** Press on row `id`'s grip at height `y`. */
function press(id: number, y = top(id) + 25, init: Partial<PointerEventInit> = {}) {
  const event = pointer('pointerdown', { clientX: 10, clientY: y, ...init })
  grips.get(id)?.dispatchEvent(event)
  return event
}

/** Move the pointer to height `y` and let the frame it asked for run. */
async function moveTo(y: number, init: Partial<PointerEventInit> = {}) {
  window.dispatchEvent(pointer('pointermove', { clientX: 10, clientY: y, ...init }))
  await frame()
}

/** Runs the animation frames asked for so far, then everything they set going. */
async function frame() {
  const due = frames
  frames = []
  for (const callback of due) callback(0)
  await vi.waitFor(() => {
    expect(true).toBe(true)
  })
  await nextTick()
}

beforeEach(() => {
  saved = []
  overs = []
  frames = []
  reduced = false
  animate = vi.fn(() => ({ cancel: vi.fn(), finished: Promise.resolve() }))
  HTMLElement.prototype.animate = animate as unknown as typeof HTMLElement.prototype.animate
  vi.stubGlobal('matchMedia', () => ({ matches: reduced }))
  scrollBy = vi.fn()
  window.scrollBy = scrollBy as unknown as typeof window.scrollBy
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => frames.push(callback))
  vi.stubGlobal('cancelAnimationFrame', () => {
    frames = []
  })

  order = ref([1, 2, 3, 4])
  scope = effectScope()
  const made = scope.run(() =>
    useSortable<number[]>({
      model: () => order.value ?? [],
      drop: (list, drag, over) => {
        overs.push(over)
        if (over.kind !== 'item') return null
        const moved = list.filter((id) => id !== drag.id)
        moved.splice(list.indexOf(over.id), 0, drag.id)
        return moved
      },
      step: (list, drag, by) => {
        const other = list[list.indexOf(drag.id) + by]
        if (other === undefined) return null
        return list.map((id) => (id === drag.id ? other : id === other ? drag.id : id))
      },
      signature: (list) => list.join(),
      describe: (list, drag) =>
        `${String(drag.id)} is ${String(list.indexOf(drag.id) + 1)} of ${String(list.length)}`,
      save: (list) => {
        saved.push(list)
        order.value = list
        return Promise.resolve()
      },
    }),
  )
  if (made === undefined) throw new Error('no sortable')
  sortable = made

  // Row 4 stands in a group, and has a foot to drop on.
  const group = document.createElement('section')
  group.dataset.sortKind = 'group'
  group.dataset.sortId = '9'
  grips = new Map()
  rows = new Map()
  for (const id of [1, 2, 3, 4]) {
    const row = document.createElement('div')
    row.dataset.sortKind = 'item'
    row.dataset.sortId = String(id)
    row.getBoundingClientRect = () => rect(id) as DOMRect
    const grip = document.createElement('button')
    grip.addEventListener('pointerdown', (event) => {
      sortable.start(event, 'item', id)
    })
    grip.addEventListener('keydown', (event) => void sortable.key(event, 'item', id))
    row.append(grip)
    ;(id === 4 ? group : document.body).append(row)
    grips.set(id, grip)
    rows.set(id, row)
  }
  document.body.append(group)
  edge = document.createElement('i')
  edge.dataset.sortEdge = 'end'
  rows.get(4)?.append(edge)
  document.elementsFromPoint = (_x, y) => {
    const found = [...rows.values()].filter((row) => {
      const box = row.getBoundingClientRect()
      return y >= box.top && y < box.bottom
    })
    return found.flatMap((row) => (row === rows.get(4) && y > 300 ? [edge, row] : [row]))
  }
})

afterEach(() => {
  scope.stop()
  document.body.replaceChildren()
  vi.unstubAllGlobals()
})

describe('dragging a grip', () => {
  it('lifts the row, opens its gap as it passes the others, and saves once on drop', async () => {
    press(1)
    expect(sortable.dragging.value).toEqual({ kind: 'item', id: 1 })
    expect(rows.get(1)?.hasAttribute('data-sort-dragging')).toBe(true)
    expect(document.body.style.userSelect).toBe('none')

    // Past row 2 and into row 3: it takes row 3's place, the others slide up.
    await moveTo(130)
    expect(sortable.shown.value).toEqual([2, 3, 1, 4])
    expect(rows.get(1)?.style.transform).toMatch(/^translate\(/)
    expect(animate).toHaveBeenCalled()
    expect(saved).toEqual([])

    await moveTo(130)
    window.dispatchEvent(pointer('pointerup', { clientY: 130 }))
    await frame()
    await vi.waitFor(() => {
      expect(saved).toEqual([[2, 3, 1, 4]])
    })
    expect(sortable.announcement.value).toBe('1 is 3 of 4')
    expect(rows.get(1)?.hasAttribute('data-sort-dragging')).toBe(false)
    expect(rows.get(1)?.style.transform).toBe('')
    expect(sortable.dragging.value).toBeNull()
    expect(document.body.style.userSelect).toBe('')
    await vi.waitFor(() => {
      expect(sortable.shown.value).toEqual([2, 3, 1, 4])
    })
  })

  it('waits for the pointer to be where the lifted row would stand before swapping', async () => {
    press(1)
    // Row 4 is 200 high: the swap needs the pointer in its last 50px, not just over it.
    await moveTo(160)
    expect(sortable.shown.value).toEqual([1, 2, 3, 4])
    await moveTo(320)
    expect(sortable.shown.value).toEqual([2, 3, 4, 1])
    // The swap back needs the pointer in the slot the lifted row would take.
    await moveTo(10)
    expect(sortable.shown.value).toEqual([1, 2, 3, 4])
  })

  it('tells the drop which edge of a box the pointer is on, and what holds it', async () => {
    press(1)
    // Row 4's body: not in its last 50px, so no swap; what holds it is offered, with no edge.
    await moveTo(160)
    expect(overs).toContainEqual({ kind: 'item', id: 4 })
    expect(overs).toContainEqual({ kind: 'group', id: 9 })
    // Its foot is an edge.
    await moveTo(310)
    expect(overs).toContainEqual({ kind: 'item', id: 4, edge: 'end' })
  })

  it('saves nothing when it is let go where it started', async () => {
    press(2)
    await moveTo(60)
    window.dispatchEvent(pointer('pointerup'))
    await frame()
    expect(saved).toEqual([])
    expect(sortable.shown.value).toEqual([1, 2, 3, 4])
    expect(sortable.dragging.value).toBeNull()
  })

  it('puts everything back on Escape or when the pointer is cancelled', async () => {
    press(1)
    await moveTo(130)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
    expect(sortable.dragging.value).not.toBeNull()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await frame()
    expect(sortable.shown.value).toEqual([1, 2, 3, 4])
    expect(saved).toEqual([])

    press(1)
    await moveTo(130)
    window.dispatchEvent(pointer('pointercancel'))
    await frame()
    expect(sortable.shown.value).toEqual([1, 2, 3, 4])
    expect(sortable.dragging.value).toBeNull()
  })

  it('follows one pointer only, and one drag at a time', async () => {
    expect(press(1, 25, { pointerType: 'mouse', button: 2 }).defaultPrevented).toBe(false)
    expect(sortable.dragging.value).toBeNull()

    press(1, 25, { pointerId: 7, pointerType: 'touch', button: 0 })
    press(2, 75)
    expect(sortable.dragging.value?.id).toBe(1)
    await moveTo(130, { pointerId: 3 })
    expect(sortable.shown.value).toEqual([1, 2, 3, 4])
    window.dispatchEvent(pointer('pointerup', { pointerId: 3 }))
    window.dispatchEvent(pointer('pointercancel', { pointerId: 3 }))
    expect(sortable.dragging.value?.id).toBe(1)
    window.dispatchEvent(pointer('pointercancel', { pointerId: 7 }))
    await frame()
    expect(sortable.dragging.value).toBeNull()
  })

  it('ignores a grip that is not in a row, and keeps the context menu from opening', () => {
    const stray = document.createElement('button')
    document.body.append(stray)
    stray.addEventListener('pointerdown', (event) => {
      sortable.start(event, 'item', 99)
    })
    stray.dispatchEvent(pointer('pointerdown'))
    expect(sortable.dragging.value).toBeNull()

    press(1)
    const menu = new Event('contextmenu', { cancelable: true })
    window.dispatchEvent(menu)
    expect(menu.defaultPrevented).toBe(true)
  })

  it('takes the latest pointer position when moves come faster than the list can follow', async () => {
    press(1)
    window.dispatchEvent(pointer('pointermove', { clientX: 10, clientY: 60 }))
    window.dispatchEvent(pointer('pointermove', { clientX: 10, clientY: 70 }))
    // Two updates at once: the second waits for the first, then runs.
    const first = frames.find((callback) => callback.name === '')
    frames = []
    first?.(0)
    window.dispatchEvent(pointer('pointermove', { clientX: 10, clientY: 130 }))
    frames[0]?.(0)
    await frame()
    await frame()
    expect(sortable.shown.value).toEqual([2, 3, 1, 4])
  })

  it('moves the rows without animating them when motion is reduced', async () => {
    reduced = true
    press(1)
    await moveTo(130)
    expect(sortable.shown.value).toEqual([2, 3, 1, 4])
    expect(animate).not.toHaveBeenCalled()
  })

  it('lets go of its listeners and animations with the scope', async () => {
    press(1)
    await moveTo(130)
    scope.stop()
    await moveTo(10)
    expect(sortable.shown.value).toEqual([2, 3, 1, 4])
  })

  it('keeps a lifted row under the pointer while its parent slides', async () => {
    animate.mockImplementation(() => ({ cancel: vi.fn(), finished: new Promise(() => undefined) }))
    press(1)
    await moveTo(130)
    expect(frames.length).toBeGreaterThan(0)
    await frame()
    await frame()
    expect(rows.get(1)?.style.transform).toMatch(/^translate\(/)
  })
})

describe('near the edge of the window', () => {
  it('scrolls up near the top and down near the bottom, and looks again at what is under it', async () => {
    press(2)
    await moveTo(10)
    expect(scrollBy).toHaveBeenLastCalledWith(0, expect.any(Number))
    expect(scrollBy.mock.lastCall?.[1]).toBeLessThan(0)
    await moveTo(window.innerHeight - 10)
    expect(scrollBy.mock.lastCall?.[1]).toBeGreaterThan(0)
    scrollBy.mockClear()
    await moveTo(300)
    expect(scrollBy).not.toHaveBeenCalled()
  })
})

describe('the keyboard on a grip', () => {
  function key(id: number, name: string) {
    const event = new KeyboardEvent('keydown', { key: name, cancelable: true, bubbles: true })
    grips.get(id)?.dispatchEvent(event)
    return event
  }

  it('moves the row, keeps focus, saves once, and says where it went', async () => {
    document.body.focus()
    const event = key(2, 'ArrowUp')
    await vi.waitFor(() => {
      expect(saved).toEqual([[2, 1, 3, 4]])
    })
    expect(event.defaultPrevented).toBe(true)
    expect(sortable.announcement.value).toBe('2 is 1 of 4')
    expect(document.activeElement).toBe(grips.get(2))
  })

  it('does nothing past the end of the list, mid-drag, or for other keys', async () => {
    expect(key(1, 'ArrowUp').defaultPrevented).toBe(true)
    expect(key(3, 'a').defaultPrevented).toBe(false)
    press(1)
    expect(key(2, 'ArrowDown').defaultPrevented).toBe(true)
    await nextTick()
    expect(saved).toEqual([])
    expect(sortable.shown.value).toEqual([1, 2, 3, 4])
  })
})
