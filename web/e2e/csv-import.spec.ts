// CSV import (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import): a fake file is mapped, previewed
// and imported into a new manual account, the same file again skips the mapping step and adds
// nothing, a file with another header brings its own mapping, and Settings › Imports undoes the
// batches. It runs last, on an account of its own, so no other spec sees its rows.
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

const fixture = (name: string) => fileURLToPath(new URL(`../../fixtures/${name}`, import.meta.url))

test.use({ locale: 'en-US' })

test('map, preview, import, import again, and undo', async ({ page }) => {
  await page.goto('/accounts')
  await page.getByRole('button', { name: 'New account' }).click()
  const form = page.getByRole('form', { name: 'New account' })
  await form.getByLabel('Name').fill('Fake file account')
  await form.getByLabel("What's in it today?").fill('100')
  await form.getByRole('button', { name: 'Create account' }).click()
  await expect(form).toBeHidden()

  // From the account's detail, with the account already chosen.
  await page.getByRole('link', { name: 'Fake file account' }).click()
  await page.getByRole('main').getByRole('link', { name: 'Import a file' }).click()
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Import a file')
  await expect(page.getByLabel('Account')).toHaveText(/Fake file account/)
  await page.getByLabel('CSV file').setInputFiles(fixture('bank-signed.csv'))

  // The header says what's where; nothing is written until Import.
  const mapping = page.getByRole('region', { name: 'Mapping' })
  await expect(mapping.getByLabel('Date column', { exact: true })).toHaveValue('0')
  await expect(mapping.getByLabel('Amount column', { exact: true })).toHaveValue('2')
  await expect(mapping.getByLabel('Date format')).toHaveValue('ymd')
  await mapping.getByRole('button', { name: 'Preview' }).click()
  const preview = page.getByRole('region', { name: 'Preview' })
  await expect(preview.getByTestId('preview-summary')).toHaveText(
    '5 new, 0 already there, 0 to check',
  )
  await expect(preview.getByRole('table', { name: 'New rows' })).toContainText('DEMO CAFE #12')
  await preview.getByRole('button', { name: 'Import' }).click()
  const done = page.getByRole('region', { name: 'Imported' })
  await expect(done.getByTestId('done-summary')).toHaveText('5 added, 0 already there, 0 skipped')

  // Imported with the right signs.
  await done.getByRole('link', { name: 'See transactions' }).click()
  await expect(page.getByText('DEMO PAYROLL').locator('visible=true').first()).toBeVisible()
  await expect(page.getByText('−€4.50').locator('visible=true').first()).toBeVisible()

  // The same file again: same header, so no mapping step, and nothing is added.
  await page.goto('/import/file')
  await page.getByLabel('Account').selectOption({ label: 'Fake file account' })
  await page.getByLabel('CSV file').setInputFiles(fixture('bank-signed.csv'))
  await expect(preview.getByTestId('preview-summary')).toHaveText(
    '0 new, 5 already there, 0 to check',
  )
  await expect(preview).toContainText('Using the mapping saved')
  await preview.getByRole('button', { name: 'Import' }).click()
  await expect(done.getByTestId('done-summary')).toHaveText('0 added, 5 already there, 0 skipped')

  // Another header brings its own mapping: debit and credit columns.
  await done.getByRole('button', { name: 'Import another file' }).click()
  await page.getByLabel('Account').selectOption({ label: 'Fake file account' })
  await page.getByLabel('CSV file').setInputFiles(fixture('bank-debit-credit.csv'))
  await expect(mapping.getByLabel('Debit column (money out)')).toHaveValue('3')
  await expect(mapping.getByLabel('Credit column (money in)')).toHaveValue('4')
  await expect(mapping.getByLabel('Date format')).toHaveValue('mdy')
  await mapping.getByRole('button', { name: 'Preview' }).click()
  await expect(preview.getByTestId('preview-summary')).toHaveText(
    '3 new, 0 already there, 0 to check',
  )
  await preview.getByRole('button', { name: 'Import' }).click()
  await expect(done.getByTestId('done-summary')).toHaveText('3 added, 0 already there, 0 skipped')

  // Undo, newest first: the debit and credit file, the one that only matched, then the first.
  await page.goto('/settings/imports')
  const batches = page.getByRole('list', { name: 'Imports' }).getByRole('listitem')
  await expect(batches.nth(0)).toContainText('CSV · bank-debit-credit.csv')
  await expect(batches.nth(1)).toContainText('0 added, 5 matched')
  await expect(batches.nth(2)).toContainText('5 added, 0 matched')
  for (const at of [0, 1, 2]) {
    await batches.nth(at).getByRole('button', { name: 'Undo', exact: true }).click()
    await expect(batches.nth(at)).toContainText('Undone')
  }
  await page.goto('/transactions')
  await expect(page.getByText('DEMO PAYROLL')).toHaveCount(0)
})
