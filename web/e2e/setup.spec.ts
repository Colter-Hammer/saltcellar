// Claims the fresh install the other tests run against: the setup wizard, end to end, with the
// setup code read from the app's log. It runs once, before the phone and desktop projects, and
// leaves its login's cookie in STORAGE_STATE for them.
import { execSync } from 'node:child_process'
import { expect, test } from '@playwright/test'
import { STORAGE_STATE } from './login-state.js'

const PASSWORD = 'correct horse battery staple'

/** The app's log so far, through the command in E2E_APP_LOGS (`just e2e` and CI set it). */
function appLog(): string {
  const command = process.env.E2E_APP_LOGS
  if (!command) throw new Error('E2E_APP_LOGS must be a command that prints the app’s log')
  return execSync(command, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
}

function setupCode(): string {
  const codes = [...appLog().matchAll(/Setup code: ([A-Z0-9]{4}-[A-Z0-9]{4})/g)]
  const code = codes.at(-1)?.[1]
  if (code === undefined) throw new Error('no setup code in the app’s log')
  return code
}

test.use({ locale: 'en-US', timezoneId: 'Asia/Tokyo' })

test('a fresh install is claimed with the setup code from the log', async ({ page, request }) => {
  const gated = await request.get('/api/v1/household')
  expect(gated.status()).toBe(403)
  expect(((await gated.json()) as { error: { code: string } }).error.code).toBe('setup_required')

  await page.goto('/accounts')
  await expect(page).toHaveURL(/\/setup$/)
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Set up Saltcellar')
  await expect(page.getByRole('navigation')).toHaveCount(0)

  await page.getByLabel('Setup code').fill('2222-2222')
  await page.getByRole('button', { name: 'Continue' }).click()
  await expect(page.getByText("That isn't the setup code.")).toBeVisible()

  const code = setupCode()
  await page.getByLabel('Setup code').fill(code)
  await page.getByRole('button', { name: 'Continue' }).click()

  await expect(page.getByLabel('Currency')).toHaveValue('USD')
  await expect(page.getByLabel('Timezone')).toHaveValue('Asia/Tokyo')
  await page.getByLabel('Currency').fill('EUR')
  await page.getByLabel('Timezone').fill('Europe/Berlin')
  await page.getByRole('button', { name: 'Continue' }).click()

  await page.getByLabel('Your name').fill('Sam Example')
  await page.getByLabel('Username').fill('sam')
  await page.getByLabel('Password', { exact: true }).fill('elevenchars')
  await page.getByLabel('Password again').fill('elevenchars')
  await page.getByRole('button', { name: 'Finish setup' }).click()
  await expect(page.getByText('Use at least 12 characters.')).toBeVisible()

  await page.getByLabel('Password', { exact: true }).fill(PASSWORD)
  await page.getByLabel('Password again').fill(`${PASSWORD}!`)
  await page.getByRole('button', { name: 'Finish setup' }).click()
  await expect(page.getByText("The passwords don't match.")).toBeVisible()

  await page.getByLabel('Password again').fill(PASSWORD)
  await page.getByRole('button', { name: 'Finish setup' }).click()

  await expect(page).toHaveURL(/\/transactions$/)
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Transactions')
  await expect(
    page.getByRole('main').getByRole('link', { name: 'Import from Actual' }),
  ).toBeVisible()
  await expect(
    page.getByRole('main').getByRole('link', { name: 'Set up categories' }),
  ).toBeVisible()

  const session = (await page.context().cookies()).find((cookie) => cookie.name === 'session')
  expect(session).toMatchObject({ httpOnly: true, sameSite: 'Lax' })

  // The household is written, the wizard and the code are done with, and the log has no secrets
  // but the code.
  const household = await request.get('/api/v1/household')
  expect(await household.json()).toEqual({ currency: 'EUR', timezone: 'Europe/Berlin' })
  const again = await request.post('/api/v1/setup/code', { data: { code } })
  expect(again.status()).toBe(409)
  await page.goto('/setup')
  await expect(page).toHaveURL(/\/transactions$/)
  const log = appLog()
  expect(log).not.toContain(PASSWORD)
  expect(log).not.toContain(session?.value)

  await page.context().storageState({ path: STORAGE_STATE })
})
