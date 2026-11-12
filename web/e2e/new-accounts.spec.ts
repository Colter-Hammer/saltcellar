// The new accounts prompt (docs/spec/m2-bank-data.md#accounts): claim the fake SimpleFIN Bridge in
// Settings › Bank connection & sync, then decide on each account it reports at the top of
// Accounts: track two, ignore one. Tracking back-fills: the checking account's rows arrive in
// Transactions. It runs last, so the new accounts are in no other spec's list.
import { expect, test } from '@playwright/test'

/** A setup token from the fake Bridge beside the app, good for one claim. */
async function setupToken(request: import('@playwright/test').APIRequestContext): Promise<string> {
  const bridge = process.env.E2E_BRIDGE_URL
  if (!bridge) throw new Error('E2E_BRIDGE_URL must be the fake SimpleFIN Bridge’s address')
  const response = await request.post(`${bridge}/_fake/token`)
  expect(response.ok()).toBe(true)
  return response.text()
}

test('claim, then track two new accounts and ignore one, and see the rows', async ({ page }) => {
  await page.goto('/settings/bank')
  await page.getByLabel('Setup token').fill(await setupToken(page.request))
  await page.getByRole('button', { name: 'Connect' }).click()
  await expect(page.getByText('SimpleFIN Bridge · connected')).toBeVisible()

  await page.goto('/accounts')
  const prompt = page.getByRole('region', { name: '3 new accounts found' })
  await expect(prompt).toBeVisible()

  // Track · budgeted: a new checking account.
  const checking = prompt.getByRole('listitem', { name: 'Demo Everyday Checking' })
  await expect(checking).toContainText('First Demo Bank')
  await checking.getByRole('button', { name: 'Track · budgeted' }).click()
  await checking
    .getByRole('form', { name: 'Track Demo Everyday Checking' })
    .getByRole('button', { name: 'Track' })
    .click()
  await expect(page.getByRole('region', { name: '2 new accounts found' })).toBeVisible()

  // Track · tracking: the card, balances only.
  const card = page.getByRole('listitem', { name: 'Demo Rewards Card' })
  await card.getByRole('button', { name: 'Track · tracking' }).click()
  const form = card.getByRole('form', { name: 'Track Demo Rewards Card' })
  await form.getByLabel(/Balances only/).check()
  await form.getByRole('button', { name: 'Track' }).click()
  await expect(page.getByRole('region', { name: '1 new account found' })).toBeVisible()

  // Ignore: it moves to Ignored, with Start tracking….
  await page
    .getByRole('listitem', { name: 'Demo Savings' })
    .getByRole('button', { name: 'Ignore' })
    .click()
  await expect(page.getByRole('region', { name: /new accounts? found/ })).toBeHidden()
  const ignored = page.getByRole('region', { name: 'Ignored' })
  await expect(
    ignored
      .getByRole('listitem', { name: 'Demo Savings' })
      .getByRole('button', { name: 'Start tracking…' }),
  ).toBeVisible()

  // The two tracked ones are accounts now, with the roles chosen.
  const table = page.getByRole('table', { name: 'Accounts' })
  const row = (name: string) =>
    table.getByRole('row').filter({ has: page.getByRole('link', { name, exact: true }) })
  await expect(row('Demo Everyday Checking')).toContainText('Checking')
  await expect(row('Demo Everyday Checking')).toContainText('Budgeted')
  await expect(row('Demo Rewards Card')).toContainText('Credit card')
  await expect(row('Demo Rewards Card')).toContainText('Tracking')
  await expect(row('Demo Savings')).toHaveCount(0)

  // Tracking back-filled the checking account: its rows are in Transactions.
  const href = await row('Demo Everyday Checking')
    .getByRole('link', { name: 'Demo Everyday Checking', exact: true })
    .getAttribute('href')
  await page.goto(`/transactions?account=${href?.split('/').pop() ?? ''}`)
  const rows = page.getByRole('table', { name: 'Transactions' }).getByRole('row')
  await expect(rows.filter({ hasText: 'Demo Employer' })).toHaveCount(1)
  await expect(rows.filter({ hasText: 'Demo Grocer' })).toHaveCount(1)

  // Sync now: the bank has nothing new.
  await page.goto('/settings/bank')
  await page.getByRole('button', { name: 'Sync now' }).click()
  await expect(page.getByRole('button', { name: 'Sync now' })).toBeEnabled()
  await expect(page.getByTestId('last-sync')).toHaveText('Last sync: No changes')

  // The schedule and the budget: two back-fills of two requests each and a Sync now, of the 100
  // e2e allows (BUDGET_SYNC_REQUEST_CAP).
  await expect(page.getByTestId('sync-schedule')).toHaveText(
    'Syncs daily around 6 AM · 95 of 100 requests left today',
  )
  const runs = page.getByRole('list', { name: 'Recent syncs' }).getByRole('listitem')
  await expect(runs).toHaveCount(3)
  await expect(runs.first()).toContainText('Sync now')
  await expect(runs.first()).toContainText('No changes · 1 request')
  await expect(runs.nth(1)).toContainText('Back-fill')

  // A new sync time reads loosely, and is kept.
  await page.getByLabel('Daily sync time').fill('07:41')
  await page.getByRole('button', { name: 'Save time' }).click()
  await expect(page.getByTestId('sync-schedule')).toContainText('Syncs daily around 8 AM')
  await page.reload()
  await expect(page.getByLabel('Daily sync time')).toHaveValue('07:41')
})
