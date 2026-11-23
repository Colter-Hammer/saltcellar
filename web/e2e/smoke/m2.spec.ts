// Milestone 2's smoke test (docs/spec/m2-bank-data.md › Done when), the main flow in one go on a
// fresh install of its own: E2E_M2_BASE_URL, started with a BUDGET_SECRET_KEY and set to claim from
// its own fake SimpleFIN Bridge at E2E_M2_BRIDGE_URL, with its log through E2E_M2_APP_LOGS. The
// setup wizard, the claim, tracking accounts (the first sync back-fills them), a CSV imported into
// a manual account, a manual row on the checking account that the fake Bridge then sends, which
// Sync now adopts, and the balances shown after each step.
import { fileURLToPath } from 'node:url'
import { expect, test, type APIRequestContext, type Page } from '@playwright/test'
import { appLog, setupCode } from '../app.js'

const fixture = fileURLToPath(new URL('../../../fixtures/bank-signed.csv', import.meta.url))

const PASSWORD = 'a smoke test passphrase'

test.use({ locale: 'en-US', timezoneId: 'Europe/Berlin' })
// The whole milestone in one test, nine page loads and two syncs.
test.setTimeout(90_000)

function bridgeUrl(): string {
  const url = process.env.E2E_M2_BRIDGE_URL
  if (!url) throw new Error('E2E_M2_BRIDGE_URL must be the smoke install’s fake SimpleFIN Bridge')
  return url
}

async function bridge(request: APIRequestContext, path: string): Promise<string> {
  const response = await request.post(`${bridgeUrl()}/_fake/${path}`)
  expect(response.ok()).toBe(true)
  return response.text()
}

async function choose(page: Page, label: string, query: string, option: string) {
  const field = page.getByLabel(label)
  await field.click()
  await field.fill(query)
  await page.getByRole('option', { name: option }).click()
  await expect(field).toHaveValue(option)
}

test('claim the bank, track, sync, import a file, enter a row and see it adopted', async ({
  page,
}) => {
  // Setup, in the Bridge's currency.
  await page.goto('/')
  await expect(page).toHaveURL(/\/setup$/)
  await page.getByLabel('Setup code').fill(setupCode(appLog('E2E_M2_APP_LOGS')))
  await page.getByRole('button', { name: 'Continue' }).click()
  await choose(page, 'Currency', 'euro', 'Euro (EUR)')
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Your name').fill('Robin Example')
  await page.getByLabel('Username').fill('robin')
  await page.getByLabel('Password', { exact: true }).fill(PASSWORD)
  await page.getByLabel('Password again').fill(PASSWORD)
  await page.getByRole('button', { name: 'Finish setup' }).click()
  await expect(page).toHaveURL(/\/transactions$/)

  // Claim: a setup token from the fake Bridge connects the install (it has its secret key).
  await page.goto('/settings/bank')
  await page.getByLabel('Setup token').fill(await bridge(page.request, 'token'))
  await page.getByRole('button', { name: 'Connect' }).click()
  await expect(page.getByText('SimpleFIN Bridge · connected')).toBeVisible()

  // Track two of the three accounts it reports, ignore the third.
  await page.goto('/accounts')
  const prompt = page.getByRole('region', { name: '3 new accounts found' })
  const checking = prompt.getByRole('listitem', { name: 'Demo Everyday Checking' })
  await checking.getByRole('button', { name: 'Track · budgeted' }).click()
  await checking
    .getByRole('form', { name: 'Track Demo Everyday Checking' })
    .getByRole('button', { name: 'Track' })
    .click()
  const card = page.getByRole('listitem', { name: 'Demo Rewards Card' })
  await card.getByRole('button', { name: 'Track · tracking' }).click()
  const form = card.getByRole('form', { name: 'Track Demo Rewards Card' })
  await form.getByLabel(/Balances only/).check()
  await form.getByRole('button', { name: 'Track' }).click()
  await page
    .getByRole('listitem', { name: 'Demo Savings' })
    .getByRole('button', { name: 'Ignore' })
    .click()
  await expect(page.getByRole('region', { name: /new accounts? found/ })).toBeHidden()

  // Sync: tracking back-filled the checking account; the balances shown are the bank's.
  const accounts = page.getByRole('table', { name: 'Accounts' })
  const row = (name: string) =>
    accounts.getByRole('row').filter({ has: page.getByRole('link', { name, exact: true }) })
  await expect(row('Demo Everyday Checking')).toContainText('€1,157.58')
  await expect(row('Demo Rewards Card')).toContainText('−€229.00')
  const href = await row('Demo Everyday Checking')
    .getByRole('link', { name: 'Demo Everyday Checking', exact: true })
    .getAttribute('href')
  const transactions = `/transactions?account=${href?.split('/').pop() ?? ''}`
  await page.goto(transactions)
  const table = page.getByRole('table', { name: 'Transactions' })
  await expect(table.getByRole('row').filter({ hasText: 'Demo Employer' })).toHaveCount(1)
  await expect(table.getByRole('row').filter({ hasText: 'Demo Grocer' })).toHaveCount(1)

  // Import a CSV into a manual account.
  await page.goto('/accounts')
  await page.getByRole('button', { name: 'New account' }).click()
  const create = page.getByRole('form', { name: 'New account' })
  await create.getByLabel('Name').fill('Fake file account')
  await create.getByLabel("What's in it today?").fill('100')
  await create.getByRole('button', { name: 'Create account' }).click()
  await expect(create).toBeHidden()
  await page.goto('/import/file')
  await page.getByLabel('Account').selectOption({ label: 'Fake file account' })
  await page.getByLabel('Bank file').setInputFiles(fixture)
  const mapping = page.getByRole('region', { name: 'Mapping' })
  await mapping.getByRole('button', { name: 'Preview' }).click()
  const preview = page.getByRole('region', { name: 'Preview' })
  await expect(preview.getByTestId('preview-summary')).toHaveText(
    '5 new, 0 already there, 0 to check',
  )
  await preview.getByRole('button', { name: 'Import' }).click()
  await expect(
    page.getByRole('region', { name: 'Imported' }).getByTestId('done-summary'),
  ).toHaveText('5 added, 0 already there, 0 skipped')

  // A manual row asks for a category: the starter set gives one.
  await page.goto('/categories')
  await page.getByRole('button', { name: 'Use starter categories' }).click()
  await expect(page.getByTestId('group-Food')).toBeVisible()

  // A manual row on the checking account, €120.00 out, ahead of the bank.
  await page.goto(transactions)
  await page.getByRole('button', { name: 'New transaction' }).click()
  const entry = page.getByRole('form', { name: 'New transaction' })
  await entry
    .getByRole('combobox', { name: 'Account', exact: true })
    .selectOption({ label: 'Demo Everyday Checking' })
  await entry.getByLabel('Amount', { exact: true }).fill('120')
  await entry.getByRole('combobox', { name: 'Payee', exact: true }).fill('Fake Landlord')
  await entry
    .getByRole('combobox', { name: 'Category', exact: true })
    .selectOption({ label: 'Groceries' })
  await entry.getByLabel('Memo').fill('check 1042')
  await entry.getByRole('button', { name: 'Enter transaction' }).click()
  await expect(entry).toBeHidden()
  await page
    .getByRole('complementary', { name: 'Fake Landlord' })
    .getByRole('button', { name: 'Close' })
    .click()
  const checks = table.getByRole('row').filter({ hasText: '−€120.00' })
  await expect(checks).toHaveCount(1)
  await expect(checks).toContainText('Manual')

  // The fake Bridge sends it: Sync now adopts the manual row, leaving one.
  await bridge(page.request, 'scenario/manual-rows-arrive')
  await bridge(page.request, 'next-day')
  await page.goto('/settings/bank')
  await page.getByRole('button', { name: 'Sync now' }).click()
  await expect(page.getByTestId('last-sync')).toHaveText('Last sync: 1 row added, 1 row updated')
  await page.goto(transactions)
  await expect(checks).toHaveCount(1)
  await expect(checks).toContainText('Fake Landlord')
  await expect(checks).not.toContainText('Manual')
  await page.screenshot({ path: 'test-results/smoke-m2-adopted.png' })

  // The balances shown still match the bank's.
  await page.goto('/accounts')
  await expect(row('Demo Everyday Checking')).toContainText('€1,157.58')
  await expect(row('Demo Rewards Card')).toContainText('−€229.00')
  await expect(row('Fake file account')).toContainText('€100.00')
  await page.screenshot({ path: 'test-results/smoke-m2-accounts.png' })
})
