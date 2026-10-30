// Transactions after the fake export is imported (import-commit.spec.ts, which runs first): every
// row reachable by scrolling the desktop table, one opened beside it, and the phone's days with a
// row opened as a sheet.
import { expect, test } from '@playwright/test'

test.use({ locale: 'en-US' })

// The fake export's rows but its five starting balances.
const ROWS = 75

test('the imported transactions, on a desktop and a phone, and one opened', async ({ page }) => {
  await page.goto('/transactions')

  // Desktop: the table, newest first, loading more on reaching the end.
  const table = page.getByRole('table', { name: 'Transactions' })
  const rows = table.locator('tbody tr')
  await expect(rows.first()).toBeVisible()
  await expect(table.getByRole('columnheader')).toHaveText([
    'Date',
    'Payee',
    'Category',
    'Account',
    'Amount',
    'Tags',
  ])
  while ((await rows.count()) < ROWS) {
    const before = await rows.count()
    await rows.last().scrollIntoViewIfNeeded()
    await expect.poll(() => rows.count()).toBeGreaterThan(before)
  }
  await expect(rows).toHaveCount(ROWS)
  await expect(page.getByRole('button', { name: 'Load more' })).toHaveCount(0)
  // Money in carries a plus, money out a minus.
  await expect(rows.filter({ hasText: /\+€/ }).first()).toBeVisible()
  await expect(rows.filter({ hasText: /-€/ }).first()).toBeVisible()

  // Open the newest: its detail beside the table.
  await rows.first().getByRole('link').click()
  await expect(page).toHaveURL(/\/transactions\/\d+$/)
  const detail = page.getByRole('complementary')
  await expect(detail.getByRole('term')).toHaveText([
    'Date',
    'Account',
    'Payee',
    'Bank description',
    'Category',
    'Memo',
    'Tags',
    'Status',
    'Source',
    'Categorized',
    'Import',
  ])
  await expect(detail.getByRole('definition').last()).toHaveText(/^actual-export\.zip, /)
  await expect(table).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page).toHaveURL(/\/transactions$/)

  // Phone: by day; a row opens as a sheet over the list, and closes back to it.
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/transactions')
  const day = page.getByRole('region').first()
  await expect(day.getByRole('heading')).toHaveText(/^\w{3} \d+, \d{4}$/)
  await day.getByRole('link').first().click()
  await expect(page.getByRole('complementary')).toBeVisible()
  await page.getByRole('button', { name: 'Close' }).click()
  await expect(page.getByRole('complementary')).toHaveCount(0)
  await expect(day).toBeVisible()
})
