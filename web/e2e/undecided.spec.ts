// From Actual, undecided after the fake export is imported (import-commit.spec.ts, which runs
// first): the rows left on Decide later wait here until they get a category. One is set from its
// detail, the groups are moved whole, and the More entry goes once none are left. It runs last,
// since the categories it sets are seen by the other specs' Transactions.
import { expect, test } from '@playwright/test'

test.use({ locale: 'en-US' })

test('the import leaves rows to decide, one is set and the groups are moved', async ({ page }) => {
  await page.goto('/more')
  await page.getByRole('main').getByRole('link', { name: 'From Actual, undecided' }).click()
  await expect(page).toHaveURL(/\/undecided$/)
  await expect(page.getByRole('heading', { name: 'From Actual, undecided' })).toBeVisible()

  const groups = page.locator('main section')
  const before = await groups.count()
  expect(before).toBeGreaterThan(0)

  // Open the first group, and one of its rows: its detail takes a category.
  const first = groups.first()
  await first.getByRole('button', { name: 'Show rows' }).click()
  const rows = first.getByRole('list').getByRole('link')
  await expect(rows.first()).toBeVisible()
  const inFirst = await rows.count()
  expect(inFirst).toBeGreaterThan(0)
  await rows.first().click()
  await expect(page).toHaveURL(/\/undecided\/\d+$/)
  const detail = page.getByRole('complementary')
  await detail.getByLabel('Category', { exact: true }).selectOption({ index: 1 })
  await detail.getByRole('button', { name: 'Set category' }).click()
  await expect(detail.getByText(/^Set by /)).toBeVisible()

  // It left its group: one fewer row there, or the whole group if it was the only one.
  if (inFirst === 1) await expect(groups).toHaveCount(before - 1)
  else await expect(first.getByRole('list').getByRole('link')).toHaveCount(inFirst - 1)

  // Move every group that's left.
  while ((await groups.count()) > 0) {
    const count = await groups.count()
    const group = groups.first()
    await group.getByLabel(/^Move all of .* to$/).selectOption({ index: 1 })
    await group.getByRole('button', { name: 'Move all' }).click()
    await expect(groups).toHaveCount(count - 1)
  }
  await expect(page.getByText('Nothing left to decide.')).toBeVisible()

  // The entry goes with the last of them.
  await page.goto('/more')
  await expect(page.getByRole('main').getByRole('link', { name: 'Settings' })).toBeVisible()
  await expect(page.getByRole('link', { name: 'From Actual, undecided' })).toHaveCount(0)
})
