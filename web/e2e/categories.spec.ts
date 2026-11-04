// Settings › Categories on both widths: the starter set, rename, hide and unhide, and reordering
// that outlasts a reload. The list is the install's own, so this runs on its own, before the
// layouts: the desktop by dragging, then a phone by Move up / Move down.
import { devices, expect, test, type Page } from '@playwright/test'
import { STORAGE_STATE } from './login-state.js'

const heading = (page: Page) => page.getByRole('heading', { level: 1 })
const groupNames = (page: Page) =>
  page.getByRole('main').locator('section[data-testid] h2').allTextContents()
const categoryNames = (page: Page, group: string) =>
  page.getByTestId(`group-${group}`).locator('li > span.flex-1').allTextContents()

/** The group header's mark: its icon's name and its dot's color as sRGB, for comparing. */
const mark = (page: Page, group: string) =>
  page.getByTestId(`group-${group}`).getByTestId('group-mark').first()
const icon = (page: Page, group: string) => mark(page, group).getAttribute('data-icon')
const color = (page: Page, group: string) =>
  mark(page, group)
    .locator('span')
    .first()
    .evaluate((dot) => {
      const canvas = document.createElement('canvas')
      canvas.width = canvas.height = 1
      const context = canvas.getContext('2d')
      if (context === null) return []
      context.fillStyle = getComputedStyle(dot).backgroundColor
      context.fillRect(0, 0, 1, 1)
      return [...context.getImageData(0, 0, 1, 1).data.slice(0, 3)]
    })

async function act(page: Page, row: string, action: string) {
  await page.getByRole('button', { name: `${row} actions`, exact: true }).click()
  await page.getByRole('menuitem', { name: action, exact: true }).click()
}

test('the starter set, renamed, hidden and reordered on a desktop and a phone', async ({
  page,
  browser,
}) => {
  await page.goto('/transactions')
  await page.getByRole('main').getByRole('link', { name: 'Set up categories' }).click()
  await expect(heading(page)).toHaveText('Categories')
  await page.getByRole('button', { name: 'Use starter categories' }).click()

  await expect
    .poll(() => groupNames(page))
    .toEqual([
      'Housing',
      'Food',
      'Transportation',
      'Health & Fitness',
      'Insurance',
      'Kids',
      'Shopping',
      'Fun',
      'Giving',
      'Pets',
      'Financial',
      'Income',
    ])
  expect(await categoryNames(page, 'Food')).toEqual([
    'Groceries',
    'Restaurants',
    'Fast Food & Coffee',
  ])
  await expect(page.getByTestId('group-Income')).toContainText('Income')

  // Each starter group has its icon and a color of its own.
  const starterIcons = {
    Housing: 'house',
    Food: 'utensils',
    Transportation: 'car',
    'Health & Fitness': 'heart-pulse',
    Insurance: 'shield',
    Kids: 'baby',
    Shopping: 'shopping-bag',
    Fun: 'party-popper',
    Giving: 'hand-heart',
    Pets: 'paw-print',
    Financial: 'landmark',
    Income: 'wallet',
  }
  const colors = new Set<string>()
  for (const [group, name] of Object.entries(starterIcons)) {
    expect(await icon(page, group)).toBe(name)
    colors.add(String(await color(page, group)))
  }
  expect(colors.size).toBe(12)
  const housingColor = await color(page, 'Housing')
  const financialColor = await color(page, 'Financial')

  // A group's icon can be changed.
  await act(page, 'Kids', 'Change icon')
  await page.getByRole('radio', { name: 'Graduation cap' }).check()
  await page
    .getByRole('form', { name: 'Icon for Kids' })
    .getByRole('button', { name: 'Save' })
    .click()
  await expect.poll(() => icon(page, 'Kids')).toBe('graduation-cap')

  // Rename a category.
  await act(page, 'Restaurants', 'Rename')
  const name = page.getByLabel('New name for Restaurants')
  await name.fill('Eating Out')
  await name.press('Enter')
  await expect(page.getByTestId('group-Food')).toContainText('Eating Out')

  // Hide a group and a category: both wait under Hidden.
  await act(page, 'Pets', 'Hide')
  await act(page, 'Fitness', 'Hide')
  const hidden = page.getByRole('region', { name: 'Hidden' })
  await expect(hidden).toContainText('Pets')
  await expect(hidden).toContainText('Fitness')
  await expect(page.getByTestId('group-Pets')).toHaveCount(0)
  // Hiding a group recolors none of the others.
  expect(await color(page, 'Financial')).toEqual(financialColor)

  // Drag a category to the top of its group, and a group above another.
  await page
    .getByLabel('Drag Fast Food & Coffee')
    .dragTo(page.getByTestId('group-Food').getByText('Groceries'))
  await expect
    .poll(() => categoryNames(page, 'Food'))
    .toEqual(['Fast Food & Coffee', 'Groceries', 'Eating Out'])
  // A drag can't scroll to its target, so both ends start on screen.
  const housing = page.getByRole('heading', { name: 'Housing' })
  await housing.scrollIntoViewIfNeeded()
  await page.getByLabel('Drag Food', { exact: true }).dragTo(housing)
  await expect.poll(async () => (await groupNames(page)).slice(0, 2)).toEqual(['Food', 'Housing'])
  // Reordering recolors: Food is first now, so it has the first color.
  await expect.poll(() => color(page, 'Food')).toEqual(housingColor)

  // A 13th group (Pets is hidden but counts) takes the first color, shifted in lightness.
  await page.getByRole('button', { name: '+ Group' }).click()
  await page.getByLabel('New group').fill('Extra')
  await page.getByRole('radio', { name: 'Gift' }).check()
  await page.getByRole('button', { name: 'Add', exact: true }).click()
  await expect.poll(() => icon(page, 'Extra')).toBe('gift')
  const extra = await color(page, 'Extra')
  const distance = Math.hypot(...extra.map((value, i) => value - (housingColor[i] ?? 0)))
  expect(distance).toBeGreaterThan(20)

  await page.reload()
  await expect
    .poll(() => categoryNames(page, 'Food'))
    .toEqual(['Fast Food & Coffee', 'Groceries', 'Eating Out'])
  expect((await groupNames(page)).slice(0, 2)).toEqual(['Food', 'Housing'])

  // A phone moves things with the row menus, and unhides.
  const phone = await (
    await browser.newContext({ ...devices['Pixel 7'], storageState: STORAGE_STATE })
  ).newPage()
  await phone.goto('/more')
  await phone.getByRole('main').getByRole('link', { name: 'Categories' }).click()
  await expect(heading(phone)).toHaveText('Categories')
  await expect(phone.getByLabel('Drag Food', { exact: true })).toBeHidden()

  await act(phone, 'Housing', 'Move up')
  await act(phone, 'Gas', 'Move down')
  await phone.getByRole('button', { name: 'Unhide Pets' }).click()
  await expect(phone.getByTestId('group-Pets')).toBeVisible()

  await phone.reload()
  await expect.poll(async () => (await groupNames(phone)).slice(0, 2)).toEqual(['Housing', 'Food'])
  expect(await categoryNames(phone, 'Transportation')).toEqual([
    'Car Maintenance',
    'Gas',
    'Car Purchase',
  ])
  expect(await groupNames(phone)).toContain('Pets')
  await expect(phone.getByRole('region', { name: 'Hidden' })).toContainText('Fitness')
  await phone.context().close()
})
