// Editing an imported account (docs/spec/m2-bank-data.md#accounts): its name and role on a desktop,
// the question that guards a role change, a taken name refused in any case, and the order, dragged
// on a desktop and on a phone and moved by keyboard. The accounts are put back at the end.
import { devices, expect, test } from '@playwright/test'
import { drag } from './drag.js'
import { STORAGE_STATE } from './login-state.js'

test.use({ locale: 'en-US' })

test('edit an imported account: name, role and order', async ({ page, browser }) => {
  await page.goto('/accounts')
  await page
    .getByRole('table', { name: 'Accounts' })
    .getByRole('link', { name: 'Rainy day savings' })
    .click()
  const detail = page.getByRole('region', { name: 'Rainy day savings' })
  await detail.getByRole('button', { name: 'Edit' }).click()

  // The dropdowns read in light and in dark: the control and its options take the page's colors,
  // not the browser's.
  for (const colorScheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme })
    for (const label of ['Type', 'Role']) {
      const colors = await page
        .getByRole('form', { name: 'Edit account' })
        .getByLabel(label)
        .evaluate((select) => {
          const own = getComputedStyle(select)
          const option = getComputedStyle(select.querySelector('option') ?? select)
          const page = getComputedStyle(document.body)
          return {
            control: [own.color, own.backgroundColor],
            option: [option.color, option.backgroundColor],
            page: [page.color, page.backgroundColor],
          }
        })
      expect(colors.control).toEqual(colors.page)
      expect(colors.option[0]).not.toEqual(colors.option[1])
    }
  }
  await page.emulateMedia({ colorScheme: null })

  // A name taken by another account, whatever its case, is a field error.
  const form = page.getByRole('form', { name: 'Edit account' })
  await form.getByLabel('Name').fill('REWARDS CARD')
  await form.getByRole('button', { name: 'Save' }).click()
  await expect(form.getByText("There's already an account called REWARDS CARD.")).toBeVisible()

  // Changing the role asks first; Cancel changes nothing.
  await form.getByLabel('Name').fill('Rainy day fund')
  await form.getByLabel('Role').selectOption('tracking')
  await form.getByRole('button', { name: 'Save' }).click()
  await expect(form.getByText('This changes what counts toward budgets.')).toBeVisible()
  await form.getByRole('button', { name: 'Cancel' }).click()
  await expect(form.getByText('This changes what counts toward budgets.')).toBeHidden()
  await form.getByRole('button', { name: 'Cancel' }).click()
  await expect(page.getByRole('region', { name: 'Rainy day savings' })).toBeVisible()
  await expect(page.getByText('Savings · Budgeted')).toBeVisible()

  // Again, and go through with it.
  await page.getByRole('button', { name: 'Edit' }).click()
  await form.getByLabel('Name').fill('Rainy day fund')
  await form.getByLabel('Role').selectOption('tracking')
  await form.getByRole('button', { name: 'Save' }).click()
  await form.getByRole('button', { name: 'Change it' }).click()
  const renamed = page.getByRole('region', { name: 'Rainy day fund' })
  await expect(renamed.getByText('Savings · Tracking')).toBeVisible()

  // The Accounts list shows both.
  await page.goto('/accounts')
  const row = page
    .getByRole('table', { name: 'Accounts' })
    .getByRole('row')
    .filter({ has: page.getByRole('rowheader', { name: 'Rainy day fund' }) })
  await expect(row.getByRole('cell').nth(2)).toHaveText('Tracking')

  // Desktop: drag Rewards card above Everyday checking. While the row is held it's lifted and the
  // other slides into the gap; the order is saved once, on drop.
  const table = page.getByRole('table', { name: 'Accounts' })
  const pair = table.getByRole('row').filter({ hasText: /Everyday checking|Rewards card/ })
  await page.goto('/accounts')
  await expect(pair).toHaveText([/Everyday checking/, /Rewards card/])
  const saves: string[] = []
  page.on('request', (request) => {
    if (request.method() === 'PUT' && request.url().endsWith('/accounts/order')) saves.push('PUT')
  })
  await drag(
    page,
    table.getByRole('button', { name: 'Reorder Rewards card' }),
    table.getByRole('row').filter({ hasText: 'Everyday checking' }),
    {
      to: 'top',
      during: async () => {
        await expect(page.locator('[data-sort-dragging]')).toHaveText(/Rewards card/)
        await expect(pair.nth(0)).toHaveText(/Rewards card/)
        expect(saves).toHaveLength(0)
      },
    },
  )
  await expect(pair).toHaveText([/Rewards card/, /Everyday checking/])
  await expect(page.locator('[data-sort-dragging]')).toHaveCount(0)
  await expect.poll(() => saves).toHaveLength(1)
  await page.reload()
  await expect(pair).toHaveText([/Rewards card/, /Everyday checking/])

  // Phone: drag it back by touch, on the grip, and the page doesn't scroll.
  const phone = await (
    await browser.newContext({ ...devices['Pixel 7'], storageState: STORAGE_STATE })
  ).newPage()
  await phone.goto('/accounts')
  const budgeted = phone.getByRole('region', { name: 'Budgeted' }).getByRole('listitem')
  await expect(budgeted).toHaveText([/Rewards card/, /Everyday checking/])
  await drag(
    phone,
    phone.getByRole('button', { name: 'Reorder Rewards card' }),
    budgeted.filter({ hasText: 'Everyday checking' }),
    { touch: true },
  )
  await expect(budgeted).toHaveText([/Everyday checking/, /Rewards card/])
  await phone.reload()
  await expect(budgeted).toHaveText([/Everyday checking/, /Rewards card/])

  // The keyboard moves a row from its grip, which keeps focus, and says where it went.
  const grip = phone.getByRole('button', { name: 'Reorder Everyday checking' })
  await grip.focus()
  await phone.keyboard.press('ArrowDown')
  await expect(budgeted).toHaveText([/Rewards card/, /Everyday checking/])
  await expect(grip).toBeFocused()
  await expect(phone.getByRole('status')).toHaveText('Everyday checking, 2 of 2 in Budgeted')
  await phone.keyboard.press('ArrowUp')
  await expect(budgeted).toHaveText([/Everyday checking/, /Rewards card/])
  await phone.close()

  // Put the account back.
  await page.goto('/accounts')
  await page.getByRole('link', { name: /Rainy day fund/ }).click()
  await page.getByRole('button', { name: 'Edit' }).click()
  await form.getByLabel('Name').fill('Rainy day savings')
  await form.getByLabel('Role').selectOption('budgeted')
  await form.getByRole('button', { name: 'Save' }).click()
  await form.getByRole('button', { name: 'Change it' }).click()
  await expect(page.getByText('Savings · Budgeted')).toBeVisible()
})
