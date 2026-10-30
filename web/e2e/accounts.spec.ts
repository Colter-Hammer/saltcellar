// Accounts after the fake export is imported (import-commit.spec.ts, which runs first): the desktop
// table and the phone's groups, with Actual's balances in the household currency, and an account's
// detail.
import { expect, test } from '@playwright/test'

test.use({ locale: 'en-US' })

test('the imported accounts, on a desktop and a phone, and one opened', async ({ page }) => {
  await page.goto('/accounts')

  // Desktop: one table, closed accounts last.
  const table = page.getByRole('table', { name: 'Accounts' })
  await expect(table.getByRole('rowheader')).toHaveText([
    'Everyday checking',
    'Rewards card',
    'Rainy day savings',
    'Brokerage',
    /Old checking\s*Closed/,
  ])
  const cells = (name: string) =>
    table
      .getByRole('row')
      .filter({ has: page.getByRole('rowheader', { name }) })
      .getByRole('cell')
  // Institution, type, role, balance, last transaction.
  await expect(cells('Everyday checking')).toHaveText([
    '—',
    'Checking',
    'Budgeted',
    '€21,458.01',
    'Dec 15, 2025',
  ])
  await expect(cells('Rewards card').nth(3)).toHaveText('-€1,363.35')
  await expect(cells('Rainy day savings').nth(3)).toHaveText('€4,000.00')
  await expect(cells('Brokerage')).toHaveText([
    '—',
    'Investment',
    'Tracking',
    '€11,000.00',
    /\d{4}$/,
  ])
  await expect(cells('Old checking').nth(3)).toHaveText('€0.00')

  // The detail: Actual's balance, and the app's own beside it, the same.
  await table.getByRole('link', { name: 'Everyday checking' }).click()
  await expect(page).toHaveURL(/\/accounts\/\d+$/)
  const detail = page.getByRole('region', { name: 'Everyday checking' })
  await expect(detail.getByText('Checking · Budgeted')).toBeVisible()
  const figures = detail.getByRole('definition')
  await expect(figures).toHaveText([
    '€21,458.01',
    '€21,458.01',
    /^€[\d,.]+ on \w{3} \d+, \d{4}$/,
    '42',
  ])
  await detail.getByRole('link', { name: 'See transactions' }).click()
  await expect(page).toHaveURL(/\/transactions\?account=\d+$/)

  // Phone: grouped Budgeted / Tracking / Closed.
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/accounts')
  const group = (name: string) => page.getByRole('region', { name })
  await expect(group('Budgeted').getByRole('listitem')).toHaveText([
    /Everyday checking\s*Checking\s*€21,458\.01/,
    /Rewards card\s*Credit card\s*-€1,363\.35/,
    /Rainy day savings\s*Savings\s*€4,000\.00/,
  ])
  await expect(group('Tracking').getByRole('listitem')).toHaveText([
    /Brokerage\s*Investment\s*€11,000\.00/,
  ])
  await expect(group('Closed').getByRole('listitem')).toHaveText([
    /Old checking\s*Checking\s*€0\.00/,
  ])
  await group('Budgeted')
    .getByRole('link', { name: /Rainy day savings/ })
    .click()
  await expect(page.getByRole('region', { name: 'Rainy day savings' })).toBeVisible()
  await page.getByRole('link', { name: 'All accounts' }).click()
  await expect(page).toHaveURL(/\/accounts$/)
})
