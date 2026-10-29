// The app shell loads from the server and its navigation reaches every milestone 1 screen.
import { expect, test, type Page } from '@playwright/test'

const heading = (page: Page) => page.getByRole('heading', { level: 1 })

test('the shell loads on Transactions, with the server reachable', async ({ page }) => {
  await page.goto('/')

  await expect(page).toHaveURL(/\/transactions$/)
  await expect(heading(page)).toHaveText('Transactions')
  await expect(page.getByRole('alert')).toHaveCount(0)
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
