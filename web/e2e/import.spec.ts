// Import from Actual: the fake export in fixtures/ is read and summed up, a zip that isn't an
// export is refused (step 1), and each account and category gets its choice (steps 2 and 3).
// Nothing is imported, so every run can upload again; only + New category… writes, a name of
// each layout's own.
import { fileURLToPath } from 'node:url'
import { expect, test, type Locator, type Page } from '@playwright/test'

const fixture = (name: string) => fileURLToPath(new URL(`../../fixtures/${name}`, import.meta.url))

test.use({ locale: 'en-US' })

async function openImporter(page: Page, isMobile: boolean) {
  await page.goto('/transactions')
  if (isMobile) {
    const tabs = page.getByRole('navigation', { name: 'Tabs' })
    await tabs.getByRole('link', { name: 'More' }).click()
    await page.getByRole('main').getByRole('link', { name: 'Import from Actual' }).click()
  } else {
    const sidebar = page.getByRole('navigation', { name: 'Sidebar' })
    await sidebar.getByRole('link', { name: 'Import from Actual' }).click()
  }
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Import from Actual')
}

test('the fake export is read and summed up', async ({ page, isMobile }) => {
  await openImporter(page, isMobile)
  const next = page.getByRole('button', { name: 'Next' })
  await expect(next).toBeDisabled()

  await page.getByLabel('Actual export (.zip)').setInputFiles(fixture('not-an-actual-export.zip'))
  await expect(page.getByRole('alert')).toHaveText(
    "This isn't an Actual export. In Actual, use Settings › Export data, and upload the .zip it saves.",
  )
  await expect(next).toBeDisabled()

  await page.getByLabel('Actual export (.zip)').setInputFiles(fixture('actual-export.zip'))
  const summary = page.getByRole('region', { name: 'The export' })
  await expect(summary.getByRole('heading')).toHaveText('Example Household')
  await expect(summary).toContainText('80 transactions · 5 accounts · 7 categories')
  await expect(summary).toContainText('Jan 1, 2025 – Jan 5, 2026')
  await expect(page.getByRole('alert')).toHaveCount(0)

  await next.click()
  await expect(page.getByRole('listitem').filter({ hasText: '2. Accounts' })).toHaveAttribute(
    'aria-current',
    'step',
  )
  await page.getByRole('button', { name: 'Back' }).click()
  await expect(summary).toBeVisible()
})

/** The option a select shows as chosen. */
const shown = (page: Page, label: string) =>
  page.getByLabel(label, { exact: true }).locator('option:checked')

test('steps 2 and 3: what happens to each account and category', async ({
  page,
  isMobile,
}, testInfo) => {
  await openImporter(page, isMobile)
  await page.getByLabel('Actual export (.zip)').setInputFiles(fixture('actual-export.zip'))
  await expect(page.getByRole('region', { name: 'The export' })).toBeVisible()
  await page.getByRole('button', { name: 'Next' }).click()

  // Step 2: every live account with its rows and balance, and what Create new starts from.
  const accounts = page.getByRole('region', { name: 'Accounts' })
  await expect(accounts.getByRole('rowheader')).toHaveText([
    'Everyday checking',
    'Rewards card',
    'Rainy day savings',
    /Brokerage\s+Off budget/,
    /Old checking\s+Closed/,
  ])
  const row = (table: Locator, name: string) =>
    table.getByRole('row').filter({ has: page.getByRole('rowheader', { name }) })
  const cells = (table: Locator, name: string) => row(table, name).getByRole('cell')
  await expect(cells(accounts, 'Everyday checking').nth(0)).toHaveText('43 rows ·')
  await expect(cells(accounts, 'Everyday checking').nth(1)).toHaveText('€21,458.01')
  await expect(cells(accounts, 'Rewards card').nth(1)).toHaveText('-€1,363.35')
  await expect(cells(accounts, 'Old checking').nth(1)).toHaveText('€0.00')
  await expect(page.getByLabel('Name for Rewards card')).toHaveValue('Rewards card')
  await expect(shown(page, 'Type for Rewards card')).toHaveText('Credit card')
  await expect(shown(page, 'Role for Rewards card')).toHaveText('Budgeted')
  await expect(shown(page, 'Type for Brokerage')).toHaveText('Investment')
  await expect(shown(page, 'Role for Brokerage')).toHaveText('Tracking')
  await expect(row(accounts, 'Old checking')).toContainText('Created closed.')
  await expect(page.getByLabel('Balances only for Brokerage')).not.toBeChecked()

  // Step 3 lists the categories rows use, with the starter categories' names pre-filled.
  const next = page.getByRole('button', { name: 'Next' })
  await next.click()
  const categories = page.getByRole('region', { name: 'Categories' })
  await expect(categories.getByRole('rowheader')).toHaveText([
    'Groceries',
    'Dining out',
    'Fuel',
    'Rent',
    'Utilities',
    'Paycheck',
  ])
  await expect(cells(categories, 'Groceries').nth(0)).toHaveText('Everyday ·')
  await expect(cells(categories, 'Groceries').nth(1)).toHaveText('15 rows')
  await expect(cells(categories, 'Paycheck').nth(0)).toHaveText('Income ·')
  await expect(shown(page, 'Groceries goes to')).toHaveText('Groceries')
  await expect(shown(page, 'Utilities goes to')).toHaveText('Utilities')
  await expect(shown(page, 'Paycheck goes to')).toHaveText('Paycheck')
  await expect(shown(page, 'Fuel goes to')).toHaveText('Decide later')
  await expect(shown(page, 'Rent goes to')).toHaveText('Decide later')
  await page.getByLabel('Dining out goes to').selectOption('Transfer (not a category)')

  // Skipping the card takes away Dining out, which only it used, and its rows elsewhere.
  await page.getByRole('button', { name: 'Back' }).click()
  await page.getByLabel('What happens to Rewards card').selectOption('Skip')
  await expect(row(accounts, 'Rewards card')).toContainText('None of its rows are imported.')
  await next.click()
  await expect(categories.getByRole('rowheader')).toHaveText([
    'Groceries',
    'Fuel',
    'Rent',
    'Utilities',
    'Paycheck',
  ])
  await expect(cells(categories, 'Groceries').nth(1)).toHaveText('1 row')

  // Back keeps every choice, and bringing the card back brings Dining out back as chosen.
  await page.getByRole('button', { name: 'Back' }).click()
  await expect(shown(page, 'What happens to Rewards card')).toHaveText('Skip')
  await page.getByLabel('What happens to Rewards card').selectOption('Create new')
  await next.click()
  await expect(shown(page, 'Dining out goes to')).toHaveText('Transfer (not a category)')
  await page.getByLabel('Dining out goes to').selectOption('Decide later')
  await expect(shown(page, 'Dining out goes to')).toHaveText('Decide later')

  // + New category… creates the category at once and chooses it. Each layout makes its own.
  const name = `Fuel (${testInfo.project.name})`
  await page.getByLabel('Fuel goes to').selectOption('+ New category…')
  const form = page.getByRole('form', { name: 'New category for Fuel' })
  await expect(form.getByLabel('New category name')).toHaveValue('Fuel')
  await form.getByLabel('New category name').fill(name)
  await form.getByLabel('In group').selectOption('Transportation')
  await form.getByRole('button', { name: 'Create' }).click()
  await expect(form).toBeHidden()
  await expect(shown(page, 'Fuel goes to')).toHaveText(name)

  // On to the last step and back: the choices are still there.
  await next.click()
  await expect(page.getByRole('listitem').filter({ hasText: '4. Done' })).toHaveAttribute(
    'aria-current',
    'step',
  )
  await page.getByRole('button', { name: 'Back' }).click()
  await expect(shown(page, 'Fuel goes to')).toHaveText(name)
  await expect(shown(page, 'Groceries goes to')).toHaveText('Groceries')
})

test('the empty Accounts screen leads to the importer', async ({ page }) => {
  await page.goto('/accounts')
  await expect(page.getByText('No accounts yet.')).toBeVisible()
  await page.getByRole('main').getByRole('link', { name: 'Import from Actual' }).click()
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Import from Actual')
})
