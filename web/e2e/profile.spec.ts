// The reset link from `budget reset-password`, then Settings › Profile: the locale and Log out
// everywhere. Both end the login's sessions, so this runs on its own, after setup and before the
// layouts, and leaves a fresh session in STORAGE_STATE for them.
import { expect, test, type Page } from '@playwright/test'
import { appLog, budget } from './app.js'
import { PASSWORD, STORAGE_STATE, USERNAME } from './login-state.js'

const heading = (page: Page) => page.getByRole('heading', { level: 1 })

async function logIn(page: Page) {
  await page.getByLabel('Username').fill(USERNAME)
  await page.getByLabel('Password').fill(PASSWORD)
  await page.getByRole('button', { name: 'Log in' }).click()
}

test('the CLI reset link works once, then the profile changes formats and logs out everywhere', async ({
  page,
  browser,
}) => {
  const unknown = budget('reset-password', 'nobody')
  expect(unknown.status).not.toBe(0)
  expect(unknown.stderr).toContain('there\'s no login named "nobody"')
  expect(unknown.stdout).toBe('')

  const issued = budget('reset-password', USERNAME)
  expect(issued.status, issued.stderr).toBe(0)
  // No BUDGET_BASE_URL here, so it prints the path and says what goes in front of it.
  expect(issued.stdout).toContain("BUDGET_BASE_URL isn't set")
  const path = /\/reset-password#([0-9a-f]{64})/.exec(issued.stdout)
  if (path === null) throw new Error(`no reset link in: ${issued.stdout}`)
  const [link, token = ''] = path

  // A second browser, logged in with the session setup made.
  const other = await (await browser.newContext({ storageState: STORAGE_STATE })).newPage()
  await other.goto('/accounts')
  await expect(heading(other)).toHaveText('Accounts')

  await page.goto(link)
  await expect(heading(page)).toHaveText('Set a new password')
  await expect(page).toHaveURL(/\/reset-password$/)
  await page.getByLabel('New password', { exact: true }).fill(PASSWORD)
  await page.getByLabel('New password again').fill(PASSWORD)
  await page.getByRole('button', { name: 'Set password and log in' }).click()
  await expect(page).toHaveURL(/\/transactions$/)

  // Once only; and the reset ended the other browser's session.
  const again = await page.context().newPage()
  await again.goto(link)
  await expect(again.getByRole('alert')).toHaveText(
    'This reset link has expired or was already used. Ask for a new one.',
  )
  await again.close()
  await other.reload()
  await expect(other).toHaveURL(/\/login\?next=/)
  await logIn(other)
  await expect(heading(other)).toHaveText('Accounts')

  // The locale: de-DE formats at once, and back.
  await page.goto('/settings')
  await page.getByRole('main').getByRole('link', { name: 'Profile' }).click()
  await expect(heading(page)).toHaveText('Profile')
  const example = page.getByTestId('format-example')
  await expect(example).toContainText('-€1,234.56')
  await expect(example).toContainText('Mar 1, 2026')
  const locale = page.getByLabel('Dates and numbers')
  await locale.click()
  await locale.fill('german germany')
  await page.getByRole('option', { name: 'German (Germany) · de-DE' }).click()
  await page.getByRole('button', { name: 'Save' }).click()
  await expect(example).toContainText('-1.234,56 €')
  await expect(example).toContainText('01.03.2026')
  await expect(example).toContainText('weeks start on Monday')
  await locale.click()
  await locale.fill('browser')
  await page.getByRole('option', { name: /^Browser default/ }).click()
  await page.getByRole('button', { name: 'Save' }).click()
  await expect(example).toContainText('-€1,234.56')

  // Log out everywhere ends the other browser's session too.
  await page.getByRole('button', { name: 'Log out everywhere' }).click()
  await expect(page).toHaveURL(/\/login$/)
  await other.reload()
  await expect(other).toHaveURL(/\/login\?next=/)
  await other.context().close()

  // The link was printed to whoever ran the command, never to the log.
  expect(appLog()).not.toContain(token)

  await logIn(page)
  await expect(page).toHaveURL(/\/transactions$/)
  await page.context().storageState({ path: STORAGE_STATE })
})
