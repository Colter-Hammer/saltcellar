// The app shell loads from the server and its navigation reaches every milestone 1 screen.
import { expect, test, type Page } from '@playwright/test'
import { smallTargets } from './look.js'

const heading = (page: Page) => page.getByRole('heading', { level: 1 })

test('the shell loads on Transactions, with the server reachable', async ({ page }) => {
  await page.goto('/')

  await expect(page).toHaveURL(/\/transactions$/)
  await expect(heading(page)).toHaveText('Transactions')
  await expect(page.getByRole('alert')).toHaveCount(0)
})

test('Accounts offers the import on a fresh install', async ({ page }) => {
  await page.goto('/accounts')

  await expect(page.getByRole('main')).toContainText('No accounts yet.')
  await page.getByRole('main').getByRole('link', { name: 'Import from Actual' }).click()
  await expect(heading(page)).toHaveText('Import from Actual')
})

test('the text is Inter, from the app itself', async ({ page, baseURL }) => {
  const fonts: string[] = []
  page.on('request', (request) => {
    if (request.resourceType() === 'font') fonts.push(request.url())
  })
  await page.goto('/')
  await expect(heading(page)).toHaveText('Transactions')
  await page.evaluate(() => document.fonts.ready)

  expect(await page.evaluate(() => getComputedStyle(document.body).fontFamily)).toMatch(
    /^"Inter Variable"/,
  )
  expect(await page.evaluate(() => document.fonts.check('16px "Inter Variable"'))).toBe(true)
  expect(fonts.length).toBeGreaterThan(0)
  for (const font of fonts) expect(new URL(font).origin).toBe(new URL(baseURL ?? '').origin)
})

test('a reload deep in the app lands back on that screen', async ({ page }) => {
  await page.goto('/settings')

  await expect(heading(page)).toHaveText('Settings')
})

test.describe('phone', () => {
  test.skip(({ isMobile }) => !isMobile, 'the tab bar is the phone layout')

  test('the tab bar and More reach every screen', async ({ page }) => {
    await page.goto('/')
    const tabs = page.getByRole('navigation', { name: 'Tabs' })

    await tabs.getByRole('link', { name: 'Accounts' }).click()
    await expect(heading(page)).toHaveText('Accounts')

    for (const screen of ['Categories', 'Import from Actual', 'Settings']) {
      await tabs.getByRole('link', { name: 'More' }).click()
      await expect(heading(page)).toHaveText('More')
      await page.getByRole('main').getByRole('link', { name: screen }).click()
      await expect(heading(page)).toHaveText(screen)
      await expect(tabs.getByRole('link', { name: 'More' })).toHaveAttribute('aria-current', 'page')
    }

    await tabs.getByRole('link', { name: 'Transactions' }).click()
    await expect(heading(page)).toHaveText('Transactions')
  })

  test('every tappable row and button is at least 44px tall', async ({ page }) => {
    for (const [path, title] of [
      ['/transactions', 'Transactions'],
      ['/accounts', 'Accounts'],
      ['/more', 'More'],
      ['/categories', 'Categories'],
      ['/import', 'Import from Actual'],
      ['/settings', 'Settings'],
      ['/settings/profile', 'Profile'],
      ['/settings/imports', 'Imports'],
    ] as const) {
      await page.goto(path)
      await expect(heading(page)).toHaveText(title)
      await page.waitForLoadState('networkidle')
      expect(await smallTargets(page), path).toEqual([])
    }
  })
})

test.describe('desktop', () => {
  test.skip(({ isMobile }) => isMobile, 'the sidebar is the desktop layout')

  test('the sidebar reaches every screen', async ({ page }) => {
    await page.goto('/')
    const sidebar = page.getByRole('navigation', { name: 'Sidebar' })

    for (const screen of [
      'Accounts',
      'Categories',
      'Import from Actual',
      'Settings',
      'Transactions',
    ]) {
      await sidebar.getByRole('link', { name: screen }).click()
      await expect(heading(page)).toHaveText(screen)
      await expect(sidebar.getByRole('link', { name: screen })).toHaveAttribute(
        'aria-current',
        'page',
      )
    }
  })
})
