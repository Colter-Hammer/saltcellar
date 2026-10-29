// Log out, fail once, log in. It starts logged out and makes a session of its own, so logging out
// leaves the shared one the other tests use alone.
import { expect, test, type Page } from '@playwright/test'
import { PASSWORD, USERNAME } from './login-state.js'

test.use({ storageState: { cookies: [], origins: [] } })

const heading = (page: Page) => page.getByRole('heading', { level: 1 })

async function logIn(page: Page, password: string) {
  await page.getByLabel('Username').fill(USERNAME)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Log in' }).click()
}

async function sessionCookie(page: Page) {
  const cookie = (await page.context().cookies()).find((c) => c.name === 'session')
  if (cookie === undefined) throw new Error('no session cookie')
  return cookie.value
}

test('log out, fail once, log in', async ({ page, request, isMobile }) => {
  await page.goto('/accounts')
  await expect(page).toHaveURL(/\/login\?next=(%2F|\/)accounts$/)
  await expect(heading(page)).toHaveText('Log in')
  await expect(page.getByRole('navigation')).toHaveCount(0)
  await logIn(page, PASSWORD)
  await expect(page).toHaveURL(/\/accounts$/)
  await expect(heading(page)).toHaveText('Accounts')
  const old = await sessionCookie(page)

  if (isMobile) {
    await page.getByRole('navigation', { name: 'Tabs' }).getByRole('link', { name: 'More' }).click()
    await page.getByRole('main').getByRole('button', { name: 'Log out' }).click()
  } else {
    await page
      .getByRole('navigation', { name: 'Sidebar' })
      .getByRole('button', { name: 'Log out' })
      .click()
  }
  await expect(page).toHaveURL(/\/login$/)
  const gone = await request.get('/api/v1/session', { headers: { Cookie: `session=${old}` } })
  expect(gone.status()).toBe(401)

  await page.goto('/accounts')
  await expect(page).toHaveURL(/\/login\?next=(%2F|\/)accounts$/)
  await logIn(page, `${PASSWORD}!`)
  await expect(page.getByText('Wrong username or password.')).toBeVisible()
  await expect(page).toHaveURL(/\/login\?next=(%2F|\/)accounts$/)

  await logIn(page, PASSWORD)
  await expect(page).toHaveURL(/\/accounts$/)
  await expect(heading(page)).toHaveText('Accounts')
})
