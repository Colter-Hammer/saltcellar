// Manual accounts (docs/spec/m2-bank-data.md#accounts, #manual-entry): Accounts › + › New account,
// what's in it today as its balance and its opening balance, dated the day before, then Update
// balance on its detail. It runs after every other spec, so the new account is in no one's list.
import { expect, test } from '@playwright/test'

test.use({ locale: 'en-US' })

/** Yesterday in the household's timezone, as the detail shows it, like "Nov 3, 2026". */
function yesterday(timeZone: string): string {
  const today = new Intl.DateTimeFormat('en-CA', { timeZone }).format(new Date())
  const [year, month, day] = today.split('-').map(Number)
  const before = new Date(Date.UTC(year ?? 0, (month ?? 1) - 1, (day ?? 1) - 1))
  return new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeZone: 'UTC' }).format(before)
}

test('create a manual account, then update its balance', async ({ page }) => {
  const household = (await (await page.request.get('/api/v1/household')).json()) as {
    timezone: string
  }

  await page.goto('/accounts')
  await page.getByRole('button', { name: 'New account' }).click()
  const form = page.getByRole('form', { name: 'New account' })
  await form.getByLabel('Name').fill('Fake wallet')
  await expect(form.getByLabel('Type')).toHaveValue('cash')
  await form.getByLabel("What's in it today?").fill('42.5')
  await form.getByRole('button', { name: 'Create account' }).click()
  await expect(form).toBeHidden()

  // On Accounts at once, with the balance entered.
  const row = page
    .getByRole('table', { name: 'Accounts' })
    .getByRole('row')
    .filter({ has: page.getByRole('link', { name: 'Fake wallet' }) })
  await expect(row).toContainText('€42.50')
  await row.getByRole('link', { name: 'Fake wallet' }).click()

  // Its opening balance is that, dated the day before.
  // The figures: the balance, the app's own, the opening balance.
  const figures = page
    .getByRole('region', { name: 'Fake wallet' })
    .getByLabel('Figures')
    .getByRole('definition')
  await expect(figures.nth(2)).toHaveText(`€42.50 on ${yesterday(household.timezone)}`)

  // Update balance: the balance shown changes; the app's own stays, the gap being drift.
  await page.getByRole('button', { name: 'Update balance' }).click()
  const update = page.getByRole('form', { name: 'Update balance' })
  await update.getByLabel('Balance today').fill('thirty')
  await update.getByRole('button', { name: 'Save balance' }).click()
  await expect(update.getByText('Enter an amount, like €1,234.56.')).toBeVisible()
  await update.getByLabel('Balance today').fill('30')
  await update.getByRole('button', { name: 'Save balance' }).click()
  await expect(update).toBeHidden()
  await expect(figures.nth(0)).toHaveText('€30.00')
  await expect(figures.nth(1)).toHaveText('€42.50')

  await page.getByRole('link', { name: '← All accounts' }).click()
  await expect(row).toContainText('€30.00')
})
