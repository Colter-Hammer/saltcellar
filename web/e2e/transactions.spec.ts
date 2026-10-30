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

test('search and filter the imported transactions, and keep them on reload', async ({ page }) => {
  await page.goto('/transactions')
  const table = page.getByRole('table', { name: 'Transactions' })
  const rows = table.locator('tbody tr')
  const column = (n: number) => rows.locator(`td:nth-child(${String(n)})`)
  await expect(rows.first()).toBeVisible()

  // A payee, then an amount: −120.00 is the fallback split.
  const search = page.getByRole('searchbox', { name: 'Search transactions' })
  await search.fill('corner grocer')
  await search.press('Enter')
  await expect(page).toHaveURL(/\?q=corner(\+|%20)grocer$/)
  await expect(column(2).first()).toHaveText('Corner Grocer')
  for (const payee of await column(2).allTextContents()) expect(payee).toContain('Corner Grocer')
  await search.fill('120')
  await search.press('Enter')
  await expect(column(5).filter({ hasText: '-€120.00' })).toHaveCount(1)

  // An account chip and a date range narrow the list.
  await search.fill('')
  await search.press('Enter')
  await page.getByRole('combobox', { name: 'Account' }).selectOption({ label: 'Everyday checking' })
  await page.getByRole('button', { name: 'Any date' }).click()
  await page.getByLabel('From', { exact: true }).fill('2025-06-01')
  await page.getByLabel('To', { exact: true }).fill('2025-06-30')
  await expect(page.getByRole('button', { name: 'Jun 1, 2025 – Jun 30, 2025' })).toBeVisible()
  await expect(page).toHaveURL(/account=\d+/)
  await expect(page).toHaveURL(/from=2025-06-01&to=2025-06-30/)
  await expect(column(1).first()).toHaveText(/^Jun \d+, 2025$/)
  for (const account of await column(4).allTextContents()) expect(account).toBe('Everyday checking')
  const filtered = await rows.allTextContents()

  // Reloading keeps every one.
  await page.reload()
  await expect(page.getByRole('combobox', { name: 'Account' })).toHaveValue(/^\d+$/)
  await expect(page.getByRole('button', { name: 'Jun 1, 2025 – Jun 30, 2025' })).toBeVisible()
  await expect(rows).toHaveText(filtered)

  await page.getByRole('button', { name: 'Clear all' }).click()
  await expect(page).toHaveURL(/\/transactions$/)
})
