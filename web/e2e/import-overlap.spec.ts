// Import from Actual over a synced account (docs/spec/m2-bank-data.md#import-from-actual-becomes-overlap-aware):
// the fake overlap export's checking account carries the fake Bridge's account id, so it starts on
// Same as the synced Demo Everyday Checking, and its rows the Bridge's transaction ids, so they
// land on the synced rows instead of coming in twice. Only the twelve older rows are added. It
// runs last, on the account new-accounts.spec.ts tracked and the specs after it synced.
import { fileURLToPath } from 'node:url'
import { type Page, expect, test } from '@playwright/test'

const fixture = (name: string) => fileURLToPath(new URL(`../../fixtures/${name}`, import.meta.url))

test.use({ locale: 'en-US' })

/** How many rows the account has, every page counted. */
async function rowCount(page: Page, account: string): Promise<number> {
  let count = 0
  let after: string | null = null
  do {
    const query = new URLSearchParams({ account, limit: '200', ...(after ? { after } : {}) })
    const response = await page.request.get(`/api/v1/transactions?${query.toString()}`)
    expect(response.ok()).toBe(true)
    const body = (await response.json()) as { transactions: unknown[]; next_cursor: string | null }
    count += body.transactions.length
    after = body.next_cursor
  } while (after !== null)
  return count
}

test('sync, then import the export that overlaps it: one copy of each row', async ({ page }) => {
  await page.goto('/accounts')
  const href = await page
    .getByRole('link', { name: 'Demo Everyday Checking', exact: true })
    .getAttribute('href')
  const checking = href?.split('/').pop() ?? ''
  const before = await rowCount(page, checking)

  await page.goto('/import')
  await page.getByLabel('Actual export (.zip)').setInputFiles(fixture('actual-export-overlap.zip'))
  await expect(page.getByRole('region', { name: 'The export' })).toContainText(
    'Example Synced Household',
  )
  await page.getByRole('button', { name: 'Next' }).click()
  const accounts = page.getByRole('region', { name: 'Accounts' })
  await expect(accounts.getByLabel('Same as for Joint checking')).toHaveValue(`app:${checking}`)
  // Savings was ignored at the bank, so it isn't offered; leave it out.
  await accounts.getByLabel('What happens to Joint savings').selectOption('skip')
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByRole('region', { name: 'Categories' })).toBeVisible()
  await page.getByRole('button', { name: 'Import' }).click()

  const done = page.getByRole('region', { name: 'Imported' })
  const summary = done.getByRole('list', { name: 'Summary' })
  await expect(summary).toContainText('0 accounts created')
  await expect(summary).toContainText('4 rows already here')
  const perAccount = done.getByRole('table', { name: 'Rows imported per account' })
  await expect(perAccount.getByRole('rowheader')).toHaveText(['Demo Everyday Checking'])
  await expect(perAccount.locator('tbody td')).toHaveText(['12', '4'])
  expect(await rowCount(page, checking)).toBe(before + 12)
})
