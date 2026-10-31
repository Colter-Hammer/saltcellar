// Settings › Imports, after the fake export is imported and its undecided rows are decided
// (import-commit.spec.ts and undecided.spec.ts run first): Undo asks first because rows were
// edited since, cancelling changes nothing, undoing empties Accounts, and the export imports
// again. It runs last, since it undoes what the other specs read.
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

const fixture = (name: string) => fileURLToPath(new URL(`../../fixtures/${name}`, import.meta.url))

test.use({ locale: 'en-US' })

test('the import is undone after a question, and imported again', async ({ page }) => {
  await page.goto('/settings/imports')
  await expect(page.getByRole('heading', { name: 'Imports' })).toBeVisible()
  const batches = page.getByRole('list', { name: 'Imports' }).getByRole('listitem')
  await expect(batches).toHaveCount(1)
  await expect(batches.first()).toContainText('actual-export.zip')
  await expect(batches.first()).toContainText('Active')

  // Rows were moved since, so Undo asks first; cancelling leaves the import in place.
  await batches.first().getByRole('button', { name: 'Undo', exact: true }).click()
  await expect(batches.first().getByRole('alert')).toContainText('edited since the import')
  await batches.first().getByRole('button', { name: 'Cancel' }).click()
  await expect(batches.first()).toContainText('Active')
  await page.goto('/accounts')
  await expect(page.getByText('No accounts yet.')).toHaveCount(0)

  await page.goto('/settings/imports')
  await batches.first().getByRole('button', { name: 'Undo', exact: true }).click()
  await batches.first().getByRole('button', { name: 'Undo anyway' }).click()
  await expect(batches.first()).toContainText('Undone')

  // Nothing of it is left, and the importer takes the export again.
  await page.goto('/accounts')
  await expect(page.getByText('No accounts yet.')).toBeVisible()
  await page.goto('/import')
  await page.getByLabel('Actual export (.zip)').setInputFiles(fixture('actual-export.zip'))
  await expect(page.getByRole('region', { name: 'The export' })).toBeVisible()
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByRole('region', { name: 'Accounts' })).toBeVisible()
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByRole('region', { name: 'Categories' })).toBeVisible()
  await page.getByRole('button', { name: 'Import' }).click()
  const done = page.getByRole('region', { name: 'Imported' })
  await expect(done.getByRole('list', { name: 'Summary' })).toContainText('5 accounts created')

  await page.goto('/settings/imports')
  await expect(batches).toHaveCount(2)
  await expect(batches.nth(0)).toContainText('Active')
  await expect(batches.nth(1)).toContainText('Undone')
})
