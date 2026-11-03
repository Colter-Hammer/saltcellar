// Milestone 1's smoke test (docs/spec/m1-ledger-import.md › Done when), the main flow in one go on
// a fresh install of its own: E2E_SMOKE_BASE_URL, with its log through E2E_SMOKE_APP_LOGS. The
// setup code from the log, the wizard, log out and in, the starter categories, the fake export
// imported with one account skipped, one + New category and one Decide later, and the rows and
// balances that come of it on Transactions and Accounts.
import { fileURLToPath } from 'node:url'
import { expect, test, type Page } from '@playwright/test'
import { appLog, setupCode } from '../app.js'

const fixture = fileURLToPath(new URL('../../../fixtures/actual-export.zip', import.meta.url))

const USERNAME = 'robin'
const PASSWORD = 'a smoke test passphrase'

test.use({ locale: 'en-US', timezoneId: 'America/Chicago' })

const heading = (page: Page) => page.getByRole('heading', { level: 1 })
const sidebar = (page: Page) => page.getByRole('navigation', { name: 'Sidebar' })
/** The option a select shows as chosen. */
const shown = (page: Page, label: string) =>
  page.getByLabel(label, { exact: true }).locator('option:checked')

test('set up, log in, import the fake export and see its rows and balances', async ({ page }) => {
  // Setup: the code from the log, the household's defaults, the first login.
  await page.goto('/')
  await expect(page).toHaveURL(/\/setup$/)
  await page.getByLabel('Setup code').fill(setupCode(appLog('E2E_SMOKE_APP_LOGS')))
  await page.getByRole('button', { name: 'Continue' }).click()
  await expect(page.getByLabel('Currency')).toHaveValue('US Dollar (USD)')
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Your name').fill('Robin Example')
  await page.getByLabel('Username').fill(USERNAME)
  await page.getByLabel('Password', { exact: true }).fill(PASSWORD)
  await page.getByLabel('Password again').fill(PASSWORD)
  await page.getByRole('button', { name: 'Finish setup' }).click()
  await expect(page).toHaveURL(/\/transactions$/)

  // Log out, and back in.
  await sidebar(page).getByRole('button', { name: 'Log out' }).click()
  await expect(page).toHaveURL(/\/login$/)
  await page.getByLabel('Username').fill(USERNAME)
  await page.getByLabel('Password').fill(PASSWORD)
  await page.getByRole('button', { name: 'Log in' }).click()
  await expect(page).toHaveURL(/\/transactions$/)

  // The starter categories.
  await page.getByRole('main').getByRole('link', { name: 'Set up categories' }).click()
  await expect(heading(page)).toHaveText('Categories')
  await page.getByRole('button', { name: 'Use starter categories' }).click()
  await expect(page.getByTestId('group-Housing')).toBeVisible()

  // Import from Actual: Brokerage skipped, Rent to a new category, Fuel left to decide later.
  await sidebar(page).getByRole('link', { name: 'Import from Actual' }).click()
  await page.getByLabel('Actual export (.zip)').setInputFiles(fixture)
  await expect(page.getByRole('region', { name: 'The export' })).toContainText(
    '80 transactions · 5 accounts · 7 categories',
  )
  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByLabel('What happens to Brokerage').selectOption('Skip')
  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByLabel('Rent goes to').selectOption('+ New category…')
  const form = page.getByRole('form', { name: 'New category for Rent' })
  await form.getByLabel('In group').selectOption('Housing')
  await form.getByRole('button', { name: 'Create' }).click()
  await expect(shown(page, 'Rent goes to')).toHaveText('Rent')
  await expect(shown(page, 'Fuel goes to')).toHaveText('Decide later')
  await page.getByRole('button', { name: 'Import' }).click()

  const done = page.getByRole('region', { name: 'Imported' })
  await expect(done.getByRole('list', { name: 'Summary' })).toContainText('4 accounts created')
  const perAccount = done.getByRole('table', { name: 'Rows imported per account' })
  await expect(perAccount.getByRole('rowheader')).toHaveText([
    'Everyday checking',
    'Rewards card',
    'Rainy day savings',
    'Old checking',
  ])
  await expect(perAccount.locator('tbody td')).toHaveText(['42', '29', '1', '2'])

  // Transactions: every imported row, Rent's under its new category.
  await done.getByRole('link', { name: 'See transactions' }).click()
  await expect(page).toHaveURL(/\/transactions$/)
  const rows = page.getByRole('table', { name: 'Transactions' }).locator('tbody tr')
  await expect(rows.first()).toBeVisible()
  while ((await rows.count()) < 74) {
    const before = await rows.count()
    await rows.last().scrollIntoViewIfNeeded()
    await expect.poll(() => rows.count()).toBeGreaterThan(before)
  }
  await expect(rows).toHaveCount(74)
  const column = (n: number) => rows.locator(`td:nth-child(${String(n)})`)
  await expect(column(3).filter({ hasText: /^Rent$/ })).toHaveCount(12)
  // The skipped account's rows are gone; the leg of its transfer in savings stays, uncategorized.
  await expect(column(4).filter({ hasText: 'Brokerage' })).toHaveCount(0)
  await expect(rows.filter({ hasText: 'Brokerage' })).toHaveText([
    /Uncategorized\s*Rainy day savings/,
  ])

  // Accounts: Actual's balances, in dollars, without the skipped one.
  await sidebar(page).getByRole('link', { name: 'Accounts' }).click()
  const table = page.getByRole('table', { name: 'Accounts' })
  await expect(table.getByRole('rowheader')).toHaveText([
    'Everyday checking',
    'Rewards card',
    'Rainy day savings',
    /Old checking\s*Closed/,
  ])
  const balance = (name: string) =>
    table
      .getByRole('row')
      .filter({ has: page.getByRole('rowheader', { name }) })
      .getByRole('cell')
      .nth(3)
  await expect(balance('Everyday checking')).toHaveText('$21,458.01')
  await expect(balance('Rewards card')).toHaveText('-$1,363.35')
  await expect(balance('Rainy day savings')).toHaveText('$4,000.00')
  await expect(balance('Old checking')).toHaveText('$0.00')
})
