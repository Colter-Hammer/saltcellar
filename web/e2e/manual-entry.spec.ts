// Manual entry (docs/spec/m2-bank-data.md#manual-entry): Transactions › + on a phone, a row
// entered with its category (which is required) and the manual badge; then on a desktop, edited,
// deleted and brought back with Undo, and a Transfer to… another account. It runs last, on the
// manual account manual-accounts.spec.ts made and the categories the import brought.
import { expect, test, type Page } from '@playwright/test'

test.use({ locale: 'en-US' })

const entryForm = (page: Page) => page.getByRole('form', { name: 'New transaction' })
const combo = (page: Page, name: string) => page.getByRole('combobox', { name, exact: true })

test('enter, edit, delete and undo, and transfer', async ({ page }) => {
  // A phone: the + opens the form as a sheet.
  await page.setViewportSize({ width: 412, height: 915 })
  await page.goto('/transactions')
  await page.getByRole('button', { name: 'New transaction' }).click()
  const form = entryForm(page)
  await combo(page, 'Account').selectOption({ label: 'Fake wallet' })
  await form.getByLabel('Amount', { exact: true }).fill('12.50')
  await combo(page, 'Payee').fill('Fake Bakery')
  await form.getByLabel('Tags').fill('e2e')
  // No category: a field error, nothing entered.
  await form.getByRole('button', { name: 'Enter transaction' }).click()
  await expect(form.getByText('Choose a category.')).toBeVisible()
  await combo(page, 'Category').selectOption({ label: 'Groceries' })
  await form.getByRole('button', { name: 'Enter transaction' }).click()
  await expect(form).toBeHidden()

  // Its detail opens, with the manual badge and its category.
  const detail = page.getByRole('complementary', { name: 'Fake Bakery' })
  await expect(detail.getByText('Manual', { exact: true })).toBeVisible()
  await expect(detail.getByLabel('Details')).toContainText('Groceries')
  await expect(detail.getByText('−€12.50')).toBeVisible()
  await detail.getByRole('button', { name: 'Close' }).click()
  const phoneRow = page.getByRole('link').filter({ hasText: 'Fake Bakery' })
  await expect(phoneRow).toContainText('Manual')
  await expect(phoneRow).toContainText('Groceries')

  // A desktop: edit every field.
  await page.setViewportSize({ width: 1280, height: 800 })
  const table = page.getByRole('table', { name: 'Transactions' })
  const row = (payee: string) => table.getByRole('row').filter({ hasText: payee })
  await expect(row('Fake Bakery')).toContainText('Manual')
  await row('Fake Bakery').getByRole('link', { name: 'Fake Bakery' }).click()
  await page.getByRole('button', { name: 'Edit' }).click()
  const edit = page.getByRole('form', { name: 'Edit transaction' })
  await edit.getByLabel('Money in').check()
  await edit.getByLabel('Amount', { exact: true }).fill('15')
  await combo(page, 'Payee').fill('Fake Bakery Refund')
  await combo(page, 'Category').selectOption({ label: 'Eating Out' })
  await edit.getByLabel('Memo').fill('returned a cake')
  await edit.getByLabel('Tags').fill('e2e edited')
  await edit.getByRole('button', { name: 'Save changes' }).click()
  await expect(edit).toBeHidden()
  const edited = page.getByRole('complementary', { name: 'Fake Bakery Refund' })
  await expect(edited.getByText('+€15.00')).toBeVisible()
  await expect(edited.getByLabel('Details')).toContainText('Eating Out')
  await expect(edited.getByLabel('Details')).toContainText('returned a cake')
  await expect(edited.getByLabel('Details')).toContainText('e2e, edited')
  await expect(row('Fake Bakery Refund')).toContainText('+€15.00')

  // Delete is immediate; Undo brings it back unchanged.
  await edited.getByRole('button', { name: 'Delete' }).click()
  const toast = page.getByRole('status').filter({ hasText: 'Transaction deleted.' })
  await expect(toast).toBeVisible()
  await expect(row('Fake Bakery Refund')).toHaveCount(0)
  await toast.getByRole('button', { name: 'Undo' }).click()
  await expect(toast).toBeHidden()
  await expect(row('Fake Bakery Refund')).toContainText('Eating Out')
  await expect(row('Fake Bakery Refund')).toContainText('+€15.00')
  await expect(row('Fake Bakery Refund')).toContainText('Fake wallet')
  await expect(row('Fake Bakery Refund')).toContainText('e2e, edited')

  // Transfer to… another account: no category asked for.
  await page.getByRole('button', { name: 'New transaction' }).click()
  const transfer = entryForm(page)
  await combo(page, 'Account').selectOption({ label: 'Fake wallet' })
  await transfer.getByLabel('Amount', { exact: true }).fill('20')
  await combo(page, 'Payee').fill('Fake top-up')
  await transfer.getByLabel('Transfer to…').check()
  await combo(page, 'Transfer to').selectOption({ label: 'Everyday checking' })
  await transfer.getByRole('button', { name: 'Enter transaction' }).click()
  await expect(transfer).toBeHidden()
  await expect(row('Fake top-up')).toContainText('Transfer to Everyday checking')
})
