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
