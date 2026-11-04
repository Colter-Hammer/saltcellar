// Editing an imported account (docs/spec/m2-bank-data.md#accounts): its name and role on a desktop,
// the question that guards a role change, a taken name refused in any case, and the order on a
// phone. The accounts are put back at the end.
import { expect, test } from '@playwright/test'

test.use({ locale: 'en-US' })

test('edit an imported account: name, role and order', async ({ page }) => {
  await page.goto('/accounts')
  await page
    .getByRole('table', { name: 'Accounts' })
    .getByRole('link', { name: 'Rainy day savings' })
    .click()
  const detail = page.getByRole('region', { name: 'Rainy day savings' })
  await detail.getByRole('button', { name: 'Edit' }).click()

  // A name taken by another account, whatever its case, is a field error.
  const form = page.getByRole('form', { name: 'Edit account' })
  await form.getByLabel('Name').fill('REWARDS CARD')
  await form.getByRole('button', { name: 'Save' }).click()
  await expect(form.getByText("There's already an account called REWARDS CARD.")).toBeVisible()

  // Changing the role asks first; Cancel changes nothing.
  await form.getByLabel('Name').fill('Rainy day fund')
  await form.getByLabel('Role').selectOption('tracking')
  await form.getByRole('button', { name: 'Save' }).click()
  await expect(form.getByText('This changes what counts toward budgets.')).toBeVisible()
  await form.getByRole('button', { name: 'Cancel' }).click()
  await expect(form.getByText('This changes what counts toward budgets.')).toBeHidden()
  await form.getByRole('button', { name: 'Cancel' }).click()
  await expect(page.getByRole('region', { name: 'Rainy day savings' })).toBeVisible()
  await expect(page.getByText('Savings · Budgeted')).toBeVisible()

  // Again, and go through with it.
  await page.getByRole('button', { name: 'Edit' }).click()
  await form.getByLabel('Name').fill('Rainy day fund')
  await form.getByLabel('Role').selectOption('tracking')
  await form.getByRole('button', { name: 'Save' }).click()
  await form.getByRole('button', { name: 'Change it' }).click()
  const renamed = page.getByRole('region', { name: 'Rainy day fund' })
  await expect(renamed.getByText('Savings · Tracking')).toBeVisible()

  // The Accounts list shows both.
  await page.goto('/accounts')
  const row = page
    .getByRole('table', { name: 'Accounts' })
    .getByRole('row')
    .filter({ has: page.getByRole('rowheader', { name: 'Rainy day fund' }) })
  await expect(row.getByRole('cell').nth(2)).toHaveText('Tracking')

  // Phone: reorder the budgeted group.
  await page.setViewportSize({ width: 390, height: 844 })
  await page.reload()
  await page.getByRole('button', { name: 'Reorder' }).click()
  const budgeted = page.getByRole('region', { name: 'Budgeted' }).getByRole('listitem')
  await expect(budgeted).toHaveText([/Everyday checking/, /Rewards card/])
  await page.getByRole('button', { name: 'Move Rewards card up' }).click()
  await expect(budgeted).toHaveText([/Rewards card/, /Everyday checking/])
  await page.reload()
  await page.getByRole('button', { name: 'Reorder' }).click()
  await expect(budgeted).toHaveText([/Rewards card/, /Everyday checking/])
  await page.getByRole('button', { name: 'Move Everyday checking up' }).click()
  await expect(budgeted).toHaveText([/Everyday checking/, /Rewards card/])

  // Put the account back.
  await page.getByRole('link', { name: /Rainy day fund/ }).click()
  await page.getByRole('button', { name: 'Edit' }).click()
  await form.getByLabel('Name').fill('Rainy day savings')
  await form.getByLabel('Role').selectOption('budgeted')
  await form.getByRole('button', { name: 'Save' }).click()
  await form.getByRole('button', { name: 'Change it' }).click()
  await expect(page.getByText('Savings · Budgeted')).toBeVisible()
})
