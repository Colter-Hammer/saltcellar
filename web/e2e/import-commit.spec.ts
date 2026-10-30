// Import from Actual, step 4: the fake export is imported with the choices steps 2 and 3 start
// from, and Done sums it up. It runs once, after both layouts: an import stays in place until it's
// undone, and while it is, another upload is refused.
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

const fixture = (name: string) => fileURLToPath(new URL(`../../fixtures/${name}`, import.meta.url))

test.use({ locale: 'en-US' })

test('the fake export is imported and summed up', async ({ page }) => {
  await page.goto('/import')
  const file = page.getByLabel('Actual export (.zip)')
  await file.setInputFiles(fixture('actual-export.zip'))
  await expect(page.getByRole('region', { name: 'The export' })).toBeVisible()
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByRole('region', { name: 'Accounts' })).toBeVisible()
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByRole('region', { name: 'Categories' })).toBeVisible()
  await page.getByRole('button', { name: 'Import' }).click()

  const done = page.getByRole('region', { name: 'Imported' })
  await expect(page.getByRole('listitem').filter({ hasText: '4. Done' })).toHaveAttribute(
    'aria-current',
    'step',
  )
  const summary = done.getByRole('list', { name: 'Summary' })
  await expect(summary).toContainText('5 accounts created')
  await expect(summary).toContainText('0 linked transfers')
  // Step 2's rows less each account's starting balance, which became its opening balance.
  const perAccount = done.getByRole('table', { name: 'Rows imported per account' })
  await expect(perAccount.getByRole('rowheader')).toHaveText([
    'Everyday checking',
    'Rewards card',
    'Rainy day savings',
    'Brokerage',
    'Old checking',
  ])
  const rows = perAccount.locator('tbody td')
  await expect(rows).toHaveText(['42', '29', '1', '1', '2'])
  await expect(done).toContainText(
    "Actual's rules weren't copied, since payee history covers them.",
  )
  await expect(done.getByRole('link', { name: 'Decide on undecided rows' })).toBeVisible()

  await done.getByRole('link', { name: 'See transactions' }).click()
  await expect(page).toHaveURL(/\/transactions$/)

  // Another import waits for this one to be undone.
  await page.goto('/import')
  await page.getByLabel('Actual export (.zip)').setInputFiles(fixture('actual-export.zip'))
  await expect(page.getByRole('alert')).toHaveText('Undo the earlier import from Actual first.')
})
