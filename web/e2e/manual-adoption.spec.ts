// Sync adopts a manual row (docs/spec/m2-bank-data.md#matching-a-synced-row step 5): a check
// entered by hand on the synced checking account, then the fake Bridge sends it and Sync now
// leaves one row, the household's, without the manual badge. It runs last, on the account
// new-accounts.spec.ts tracked and the categories the import brought, and puts the fake back on
// its steady feed after.
import { expect, test, type APIRequestContext } from '@playwright/test'

test.use({ locale: 'en-US' })

/** Makes the fake Bridge send the rows the household may have entered first. */
async function bridge(request: APIRequestContext, path: string): Promise<void> {
  const url = process.env.E2E_BRIDGE_URL
  if (!url) throw new Error('E2E_BRIDGE_URL must be the fake SimpleFIN Bridge’s address')
  const response = await request.post(`${url}/_fake/${path}`)
  expect(response.ok()).toBe(true)
}

test('a manual check, sent by the bank, is one row after Sync now', async ({ page }) => {
  await page.goto('/accounts')
  const href = await page
    .getByRole('link', { name: 'Demo Everyday Checking', exact: true })
    .getAttribute('href')
  const transactions = `/transactions?account=${href?.split('/').pop() ?? ''}`

  // Today, €120.00 out: the check the fake will send dated yesterday.
  await page.goto(transactions)
  await page.getByRole('button', { name: 'New transaction' }).click()
  const form = page.getByRole('form', { name: 'New transaction' })
  await form
    .getByRole('combobox', { name: 'Account', exact: true })
    .selectOption({ label: 'Demo Everyday Checking' })
  await form.getByLabel('Amount', { exact: true }).fill('120')
  await form.getByRole('combobox', { name: 'Payee', exact: true }).fill('Fake Landlord')
  await form
    .getByRole('combobox', { name: 'Category', exact: true })
    .selectOption({ label: 'Groceries' })
  await form.getByLabel('Memo').fill('check 1042')
  await form.getByRole('button', { name: 'Enter transaction' }).click()
  await expect(form).toBeHidden()
  await page
    .getByRole('complementary', { name: 'Fake Landlord' })
    .getByRole('button', { name: 'Close' })
    .click()

  const table = page.getByRole('table', { name: 'Transactions' })
  const checks = table.getByRole('row').filter({ hasText: '−€120.00' })
  await expect(checks).toHaveCount(1)
  await expect(checks).toContainText('Manual')

  try {
    await bridge(page.request, 'scenario/manual-rows-arrive')
    await bridge(page.request, 'next-day')
    await page.goto('/settings/bank')
    await page.getByRole('button', { name: 'Sync now' }).click()
    await expect(page.getByTestId('last-sync')).toHaveText('Last sync: 1 row added, 1 row updated')
  } finally {
    await bridge(page.request, 'scenario/steady')
  }

  // One row: the household's payee, category and memo, the badge gone.
  await page.goto(transactions)
  await expect(checks).toHaveCount(1)
  await expect(checks).toContainText('Fake Landlord')
  await expect(checks).toContainText('Groceries')
  await expect(checks).not.toContainText('Manual')
  await expect(table.getByRole('row').filter({ hasText: 'Demo Pay App' })).toHaveCount(1)
})
