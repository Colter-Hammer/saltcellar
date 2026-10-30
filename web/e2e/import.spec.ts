// Import from Actual, step 1: the fake export in fixtures/ is read and summed up, and a zip that
// isn't an export is refused. Nothing is imported, so every run can upload again.
import { fileURLToPath } from 'node:url'
import { expect, test, type Page } from '@playwright/test'

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

test('the empty Accounts screen leads to the importer', async ({ page }) => {
  await page.goto('/accounts')
  await expect(page.getByText('No accounts yet.')).toBeVisible()
  await page.getByRole('main').getByRole('link', { name: 'Import from Actual' }).click()
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Import from Actual')
})
