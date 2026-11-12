// Sync flags on rows and Recently dropped by bank (docs/spec/m2-bank-data.md#sync-health-in-the-ui):
// the fake Bridge re-issues two ids at once and sends a payment a manual entry undershot, and
// each flag is resolved from Transactions › Needs a look; then an edited pending row the bank
// drops is restored. It runs after manual-adoption.spec.ts on the tracked checking account,
// switching the fake's scenario, and puts it back on its steady feed after each test.
import { expect, test, type APIRequestContext, type Page } from '@playwright/test'

test.use({ locale: 'en-US' })
test.describe.configure({ mode: 'serial' })

/** Makes the fake Bridge send another scenario, or move on a day. */
async function bridge(request: APIRequestContext, path: string): Promise<void> {
  const url = process.env.E2E_BRIDGE_URL
  if (!url) throw new Error('E2E_BRIDGE_URL must be the fake SimpleFIN Bridge’s address')
  const response = await request.post(`${url}/_fake/${path}`)
  expect(response.ok()).toBe(true)
}

/** Sync now, through the API, so each run is waited for. */
async function syncNow(page: Page): Promise<void> {
  const response = await page.request.post('/api/v1/bank-connection/sync', { data: {} })
  expect(response.ok()).toBe(true)
  expect(((await response.json()) as { error: unknown }).error).toBeNull()
}

/** The scenario's day 0, then its day 1, each synced; steady again after. */
async function twoDays(page: Page, scenario: string, before?: () => Promise<void>) {
  try {
    await bridge(page.request, `scenario/${scenario}`)
    await syncNow(page)
    await before?.()
    await bridge(page.request, 'next-day')
    await syncNow(page)
  } finally {
    await bridge(page.request, 'scenario/steady')
  }
}

const table = (page: Page) => page.getByRole('table', { name: 'Transactions' })
const rows = (page: Page, text: string) => table(page).getByRole('row').filter({ hasText: text })

test('two ids re-issued at once are merged one by one from Needs a look', async ({ page }) => {
  await twoDays(page, 'ids-reissued-together')

  await page.goto('/transactions')
  await page.getByRole('button', { name: 'Needs a look' }).click()
  await expect(page).toHaveURL(/needs_a_look=true/)
  const parking = rows(page, 'Demo Parking')
  await expect(parking).toHaveCount(2)
  await expect(parking.first()).toContainText('Duplicate?')

  for (const left of [1, 0]) {
    await parking.first().getByRole('link', { name: 'Demo Parking' }).click()
    const detail = page.getByRole('complementary', { name: 'Demo Parking' })
    await expect(detail.getByRole('heading', { name: 'Possible duplicate' })).toBeVisible()
    await expect(detail).toContainText('Demo Parking, −€6.00')
    await detail.getByRole('button', { name: 'Merge' }).click()
    await expect(detail.getByRole('heading', { name: 'Possible duplicate' })).toBeHidden()
    await expect(parking).toHaveCount(left)
  }

  // One row for each payment, as before the bank re-issued them.
  await page.getByRole('button', { name: 'Needs a look' }).click()
  await expect(rows(page, 'Demo Parking')).toHaveCount(2)
  await expect(rows(page, 'Demo Parking').filter({ hasText: 'Duplicate?' })).toHaveCount(0)
})

test('Yes to a possible manual match keeps the bank amount and the household’s fields', async ({
  page,
}) => {
  await page.goto('/transactions')
  await page.getByRole('button', { name: 'New transaction' }).click()
  const form = page.getByRole('form', { name: 'New transaction' })
  await form
    .getByRole('combobox', { name: 'Account', exact: true })
    .selectOption({ label: 'Demo Everyday Checking' })
  await form.getByLabel('Amount', { exact: true }).fill('50')
  await form.getByRole('combobox', { name: 'Payee', exact: true }).fill('Fake Tutor')
  await form
    .getByRole('combobox', { name: 'Category', exact: true })
    .selectOption({ label: 'Groceries' })
  await form.getByLabel('Memo').fill('maths')
  await form.getByRole('button', { name: 'Enter transaction' }).click()
  await expect(form).toBeHidden()

  await twoDays(page, 'manual-match-arrives')

  await page.goto('/transactions?needs_a_look=true')
  const bankRow = rows(page, 'Demo Pay App').filter({ hasText: 'Manual match?' })
  await expect(bankRow).toHaveCount(1)
  await bankRow.getByRole('link', { name: 'Demo Pay App' }).click()
  const detail = page.getByRole('complementary', { name: 'Demo Pay App' })
  await expect(
    detail.getByRole('heading', { name: 'Is this your €50.00 manual entry?' }),
  ).toBeVisible()
  await detail.getByRole('button', { name: 'Yes' }).click()

  // One row: the bank's amount, the household's payee, category and memo.
  const merged = page.getByRole('complementary', { name: 'Fake Tutor' })
  await expect(merged.getByLabel('Details')).toContainText('Groceries')
  await expect(merged.getByLabel('Details')).toContainText('maths')
  await expect(merged).toContainText('−€57.50')
  await page.goto('/transactions')
  await expect(rows(page, 'Fake Tutor')).toHaveCount(1)
  await expect(rows(page, 'Fake Tutor')).not.toContainText('Manual')
})

test('an edited pending row the bank drops is restored from Recently dropped by bank', async ({
  page,
}) => {
  await twoDays(page, 'pending-dropped', async () => {
    // The household excludes the hotel hold from budgets: an edit.
    await page.goto('/transactions')
    await rows(page, 'Demo Hotel').getByRole('link', { name: 'Demo Hotel' }).click()
    const detail = page.getByRole('complementary', { name: 'Demo Hotel' })
    await detail.getByRole('button', { name: 'Delete' }).click()
    await detail.getByRole('button', { name: 'Exclude from budgets instead' }).click()
    await expect(detail.getByLabel('Details')).toContainText('Excluded')
  })
  // Still on pending-dropped's day 1 for the second miss.
  try {
    await bridge(page.request, 'scenario/pending-dropped')
    await bridge(page.request, 'next-day')
    await syncNow(page)
  } finally {
    await bridge(page.request, 'scenario/steady')
  }

  await page.goto('/transactions')
  await expect(rows(page, 'Demo Hotel')).toHaveCount(0)
  await page.getByRole('button', { name: 'Recently dropped by bank' }).click()
  const dropped = page.getByRole('region', { name: 'Recently dropped by bank' })
  const hold = dropped.getByRole('listitem').filter({ hasText: 'Demo Hotel' })
  await expect(hold).toContainText('−€150.00')
  await hold.getByRole('button', { name: 'Restore' }).click()

  const detail = page.getByRole('complementary', { name: 'Demo Hotel' })
  await expect(detail.getByLabel('Details')).toContainText('Excluded')
  await expect(detail.getByLabel('Details')).toContainText('Pending')
})
