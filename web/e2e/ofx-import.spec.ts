// OFX import (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import): a fake QFX (OFX 1.x) picks its
// account by the last 4 digits of the file's account id and imports with no mapping step, the same
// file again adds nothing, an OFX 2.x file whose digits match no account asks for one, and
// Settings › Imports undoes the batches. It runs after the CSV import, on accounts of its own.
import { fileURLToPath } from 'node:url'
import { type Page, expect, test } from '@playwright/test'

const fixture = (name: string) => fileURLToPath(new URL(`../../fixtures/${name}`, import.meta.url))

test.use({ locale: 'en-US' })

async function newAccount(page: Page, name: string) {
  await page.goto('/accounts')
  await page.getByRole('button', { name: 'New account' }).click()
  const form = page.getByRole('form', { name: 'New account' })
  await form.getByLabel('Name').fill(name)
  await form.getByLabel("What's in it today?").fill('0')
  await form.getByRole('button', { name: 'Create account' }).click()
  await expect(form).toBeHidden()
}

test('pre-select by the last 4 digits, import, import again, and undo', async ({ page }) => {
  await newAccount(page, 'Fake OFX card 0042')
  await newAccount(page, 'Fake OFX checking')

  // No account chosen: the file's digits choose it, and there's no mapping step.
  await page.goto('/import/file')
  await page.getByLabel('Bank file').setInputFiles(fixture('bank-v1.qfx'))
  const preview = page.getByRole('region', { name: 'Preview' })
  await expect(preview.getByTestId('preview-summary')).toHaveText(
    '3 new, 0 already there, 0 to check',
  )
  await expect(preview).toContainText(
    "Into Fake OFX card 0042, chosen by the file's account ending 0042.",
  )
  await expect(page.getByRole('list', { name: 'Steps' })).not.toContainText('Mapping')
  await expect(preview.getByRole('table', { name: 'New rows' })).toContainText('DEMO GROCER & DELI')
  await preview.getByRole('button', { name: 'Import' }).click()
  const done = page.getByRole('region', { name: 'Imported' })
  await expect(done.getByTestId('done-summary')).toHaveText('3 added, 0 already there, 0 skipped')

  // The same file again: every FITID is there already.
  await done.getByRole('button', { name: 'Import another file' }).click()
  await page.getByLabel('Bank file').setInputFiles(fixture('bank-v1.qfx'))
  await expect(preview.getByTestId('preview-summary')).toHaveText(
    '0 new, 3 already there, 0 to check',
  )
  await preview.getByRole('button', { name: 'Import' }).click()
  await expect(done.getByTestId('done-summary')).toHaveText('0 added, 3 already there, 0 skipped')

  // An OFX 2.x file for an account no name ends in: the household chooses.
  await page.goto('/import/file')
  await page.getByLabel('Bank file').setInputFiles(fixture('bank-v2.ofx'))
  const file = page.getByRole('group', { name: 'OFX file' })
  await expect(file).toContainText('4 transactions')
  await expect(file).toContainText('No one account’s name ends in 0077')
  await expect(file.getByRole('button', { name: 'Preview' })).toBeDisabled()
  await page.getByLabel('Account').selectOption({ label: 'Fake OFX checking' })
  await file.getByRole('button', { name: 'Preview' }).click()
  await expect(preview.getByTestId('preview-summary')).toHaveText(
    '4 new, 0 already there, 0 to check',
  )
  await expect(preview).toContainText('Into Fake OFX checking.')
  await preview.getByRole('button', { name: 'Import' }).click()
  await expect(done.getByTestId('done-summary')).toHaveText('4 added, 0 already there, 0 skipped')

  await page.goto('/settings/imports')
  const batches = page.getByRole('list', { name: 'Imports' }).getByRole('listitem')
  await expect(batches.nth(0)).toContainText('OFX · bank-v2.ofx')
  await expect(batches.nth(1)).toContainText('OFX · bank-v1.qfx')
  await expect(batches.nth(1)).toContainText('0 added, 3 matched')
  await expect(batches.nth(2)).toContainText('3 added, 0 matched')
  for (const at of [0, 1, 2]) {
    await batches.nth(at).getByRole('button', { name: 'Undo', exact: true }).click()
    await expect(batches.nth(at)).toContainText('Undone')
  }
  await page.goto('/transactions')
  await expect(page.getByText('DEMO GROCER & DELI')).toHaveCount(0)
})
