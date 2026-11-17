// Sync health in the UI (docs/spec/m2-bank-data.md#sync-health-in-the-ui): with the fake Bridge
// saying a bank needs signing in again, the phone's pill and the desktop sidebar's button show
// "1 sync problem", the sheet names the account and its fix, and Done, sync now clears it. It
// runs after sync-errors, on the account that spec tracked, and leaves the fake steady.
import { expect, test, type APIRequestContext, type Page } from '@playwright/test'

test.use({ locale: 'en-US' })
test.describe.configure({ mode: 'serial' })

async function bridge(request: APIRequestContext, path: string): Promise<void> {
  const url = process.env.E2E_BRIDGE_URL
  if (!url) throw new Error('E2E_BRIDGE_URL must be the fake SimpleFIN Bridge’s address')
  const response = await request.post(`${url}/_fake/${path}`)
  expect(response.ok()).toBe(true)
}

async function syncNow(page: Page): Promise<void> {
  const response = await page.request.post('/api/v1/bank-connection/sync', { data: {} })
  expect(response.ok()).toBe(true)
}

const spot = (page: Page) => page.getByTestId('sync-spot').filter({ visible: true })

test.afterAll(async ({ request }) => {
  await bridge(request, 'scenario/steady')
})

test('all good: a hollow dot and the sync time', async ({ page }) => {
  await page.goto('/accounts')
  await expect(spot(page)).toHaveText(/^\s*Synced \d{1,2}:\d{2}\s?[AP]M$/)
  await expect(spot(page)).toHaveAttribute('data-problem', 'false')
})

test('a broken bank link: the pill, the sheet and its fix, on a phone and on a desktop', async ({
  page,
}) => {
  await bridge(page.request, 'scenario/needs-auth')
  await syncNow(page)

  // Desktop: the sidebar button.
  await page.goto('/accounts')
  await expect(spot(page)).toHaveText('1 sync problem')
  await expect(spot(page)).toHaveAttribute('data-variant', 'button')

  // Phone: the header pill.
  await page.setViewportSize({ width: 390, height: 800 })
  await expect(spot(page)).toHaveText('1 sync problem')
  await expect(spot(page)).toHaveAttribute('data-variant', 'pill')
  await page.screenshot({ path: 'test-results/sync-health-phone.png' })

  await spot(page).click()
  const sheet = page.getByRole('dialog', { name: 'Bank sync' })
  const problem = sheet.getByTestId('sync-problem')
  await expect(problem).toHaveCount(1)
  await expect(problem).toContainText('Demo Savings')
  await expect(problem).toContainText('Needs reconnecting')
  await expect(problem.getByRole('link', { name: 'Fix at SimpleFIN ↗' })).toHaveAttribute(
    'href',
    /^https:\/\/bridge\.simplefin\.org\//,
  )
  await expect(sheet.getByTestId('sync-requests')).toContainText('requests left today')
  await page.screenshot({ path: 'test-results/sync-health-sheet.png' })

  // Fixed at SimpleFIN: Done, sync now clears the problem and the pill.
  await bridge(page.request, 'scenario/steady')
  await problem.getByRole('button', { name: 'Done, sync now' }).click()
  await expect(sheet.getByTestId('sync-problem')).toHaveCount(0)
  await sheet.getByRole('button', { name: 'Close' }).click()
  await expect(spot(page)).toHaveText(/^\s*\d{1,2}:\d{2}\s?[AP]M$/)
})
