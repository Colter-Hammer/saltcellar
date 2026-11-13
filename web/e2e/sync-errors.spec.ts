// Errors, reconnecting a bank and a paused connection (docs/spec/m2-bank-data.md#errors): the fake
// Bridge says a bank needs signing in again, and the Banks list in Settings shows it, the account
// carries a ⚠, and Done, sync now clears it; then it answers 403 and 402, and the paused banner
// shows on every screen until a new setup token is pasted. It runs last, on the accounts the specs
// before it tracked, switching the fake's scenario, and leaves it steady and the connection
// working.
import { expect, test, type APIRequestContext, type Page } from '@playwright/test'

test.use({ locale: 'en-US' })
test.describe.configure({ mode: 'serial' })

/** Makes the fake Bridge send another scenario. */
async function bridge(request: APIRequestContext, path: string): Promise<void> {
  const url = process.env.E2E_BRIDGE_URL
  if (!url) throw new Error('E2E_BRIDGE_URL must be the fake SimpleFIN Bridge’s address')
  const response = await request.post(`${url}/_fake/${path}`)
  expect(response.ok()).toBe(true)
}

/** A setup token from the fake Bridge, good for one claim. */
async function setupToken(request: APIRequestContext): Promise<string> {
  const url = process.env.E2E_BRIDGE_URL
  if (!url) throw new Error('E2E_BRIDGE_URL must be the fake SimpleFIN Bridge’s address')
  const response = await request.post(`${url}/_fake/token`)
  expect(response.ok()).toBe(true)
  return response.text()
}

/** Sync now, through the API, so each run is waited for. */
async function syncNow(page: Page): Promise<void> {
  const response = await page.request.post('/api/v1/bank-connection/sync', { data: {} })
  expect(response.ok()).toBe(true)
}

const banner = (page: Page) => page.getByTestId('paused-banner')
const bankRow = (page: Page, name: string) =>
  page.getByRole('list', { name: 'Banks' }).getByRole('listitem').filter({ hasText: name })

test('a bank that needs signing in again shows in Banks and on its account, and Done, sync now clears it', async ({
  page,
}) => {
  // The second bank's savings account, tracked as balances only: it has no rows, so while its
  // bank link is broken the household can enter its balance.
  const tracked = await page.request.post('/api/v1/connection-accounts/ACT-demo-savings/track', {
    data: { role: 'budgeted', balances_only: true },
  })
  expect(tracked.ok()).toBe(true)
  const accountId = ((await tracked.json()) as { account_id: number }).account_id

  await page.goto('/settings/bank')
  await expect(bankRow(page, 'Demo Credit Union')).toContainText('1 account')
  await expect(bankRow(page, 'Demo Credit Union')).toContainText('Balances only')

  await bridge(page.request, 'scenario/needs-auth')
  await syncNow(page)
  await page.reload()

  const row = bankRow(page, 'Demo Credit Union')
  await expect(row).toContainText('Needs reconnecting')
  await expect(row).toContainText('Demo Credit Union needs you to sign in again.')
  const fix = row.getByRole('link', { name: 'Fix at SimpleFIN ↗' })
  await expect(fix).toHaveAttribute('href', /^https:\/\/bridge\.simplefin\.org\//)
  await expect(fix).toHaveAttribute('target', '_blank')
  await expect(bankRow(page, 'First Demo Bank')).not.toContainText('Needs')
  // The connection itself works: no banner.
  await expect(banner(page)).toBeHidden()

  // The account carries a ⚠ in the list, and its screen says why and offers a manual balance.
  await page.goto('/accounts')
  const link = page
    .getByRole('table', { name: 'Accounts' })
    .getByRole('row')
    .filter({ has: page.getByRole('link', { name: 'Demo Savings', exact: true }) })
  await expect(link.getByRole('img', { name: 'Needs attention' })).toBeVisible()
  await page.goto(`/accounts/${String(accountId)}`)
  const sync = page.getByTestId('account-sync')
  await expect(sync).toContainText('Demo Credit Union needs you to sign in again.')
  await expect(page.getByRole('button', { name: 'Update balance' })).toBeVisible()

  // Fixed at SimpleFIN: Done, sync now syncs at once and the state clears.
  await bridge(page.request, 'scenario/steady')
  await sync.getByRole('button', { name: 'Done, sync now' }).click()
  await expect(sync).toBeHidden()
  await page.goto('/settings/bank')
  await expect(bankRow(page, 'Demo Credit Union')).toContainText('Balances only')
  await expect(page.getByRole('link', { name: 'Fix at SimpleFIN ↗' })).toHaveCount(0)
  // A working link takes no manual balance.
  await page.goto(`/accounts/${String(accountId)}`)
  await expect(page.getByRole('button', { name: 'Update balance' })).toBeHidden()
})

test('stale data is quiet in Banks, with no ⚠ and no fix', async ({ page }) => {
  try {
    await bridge(page.request, 'scenario/stale-data')
    await syncNow(page)
    await page.goto('/settings/bank')
    const row = bankRow(page, 'Demo Credit Union')
    await expect(row).toContainText('Stale')
    await expect(row).toContainText('Bank hasn’t updated in 3 days.'.replace('’', "'"))
    await expect(page.getByRole('link', { name: 'Fix at SimpleFIN ↗' })).toHaveCount(0)
    await page.goto('/accounts')
    await expect(page.getByRole('img', { name: 'Needs attention' })).toHaveCount(0)
  } finally {
    await bridge(page.request, 'scenario/steady')
    await syncNow(page)
  }
})

for (const [scenario, text] of [
  ['revoked', 'Bank sync stopped. Get a new setup token from SimpleFIN and paste it in Settings.'],
  [
    'unpaid',
    'SimpleFIN subscription unpaid. Bank sync stopped. Pay it, then paste a new setup token in Settings.',
  ],
] as const) {
  test(`${scenario}: the paused banner shows on every screen until a new setup token`, async ({
    page,
  }) => {
    await bridge(page.request, `scenario/${scenario}`)
    await syncNow(page)

    for (const path of [
      '/transactions',
      '/accounts',
      '/categories',
      '/settings',
      '/settings/bank',
    ]) {
      await page.goto(path)
      await expect(banner(page), path).toHaveText(text)
    }
    // Settings says so too, with the way out, and Sync now is gone.
    await expect(page.getByTestId('connection-state')).toHaveText('SimpleFIN Bridge · Sync paused')
    await expect(page.getByRole('button', { name: 'Sync now' })).toHaveCount(0)
    // Syncing, by any other means, is refused.
    const refused = await page.request.post('/api/v1/bank-connection/sync', { data: {} })
    expect(refused.status()).toBe(409)

    // The banner's link goes to Settings.
    await page.goto('/accounts')
    await banner(page).getByRole('link', { name: 'Settings' }).click()
    await expect(page).toHaveURL(/\/settings\/bank$/)

    // A new setup token clears it.
    await bridge(page.request, 'scenario/steady')
    await page.getByLabel('New setup token').fill(await setupToken(page.request))
    await page.getByRole('button', { name: 'Replace' }).click()
    await expect(page.getByTestId('connection-state')).toHaveText('SimpleFIN Bridge · connected')
    await expect(banner(page)).toBeHidden()
    await page.goto('/transactions')
    await expect(banner(page)).toBeHidden()
  })
}
