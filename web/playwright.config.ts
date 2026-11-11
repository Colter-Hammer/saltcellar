// End-to-end tests (docs/standards.md#tests), run against the app image: `just e2e` starts it with
// compose, CI with `docker run`. Both pass its address in E2E_BASE_URL, a command that prints its
// log in E2E_APP_LOGS, where the setup test finds the setup code, and a command that runs `budget`
// in it in E2E_APP_EXEC, for the reset link. The milestone smoke tests in e2e/smoke/ claim a fresh
// install of their own, at E2E_SMOKE_BASE_URL with its log in E2E_SMOKE_APP_LOGS, so they run
// beside the rest without sharing its state. Beside the main install runs a fake SimpleFIN Bridge
// it can claim, whose setup tokens the tests mint at E2E_BRIDGE_URL.
import { defineConfig, devices } from '@playwright/test'
import { STORAGE_STATE } from './e2e/login-state.js'

export default defineConfig({
  testDir: 'e2e',
  forbidOnly: !!process.env.CI,
  // No retries: the setup test claims the install once, so a retry would find it claimed.
  retries: 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost:8080',
    trace: 'retain-on-failure',
  },
  projects: [
    // Each milestone's main flow, from the setup wizard on, on an install of its own.
    {
      name: 'smoke',
      testMatch: 'smoke/*.spec.ts',
      use: {
        ...devices['Desktop Chrome'],
        baseURL: process.env.E2E_SMOKE_BASE_URL ?? 'http://localhost:8081',
      },
    },
    // The fresh install is claimed first, through the setup wizard, on a phone.
    { name: 'setup', testMatch: 'setup.spec.ts', use: { ...devices['Pixel 7'] } },
    // Then the reset link and the profile, on their own: they end sessions, the shared one among
    // them, and leave a new one in STORAGE_STATE.
    {
      name: 'profile',
      testMatch: 'profile.spec.ts',
      dependencies: ['setup'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then the categories, on their own too: the list is the install's, and the test works it on
    // both widths in turn.
    {
      name: 'categories',
      testMatch: 'categories.spec.ts',
      dependencies: ['profile'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then the two layouts: a phone (tab bar) and a desktop (sidebar), both in Chromium.
    {
      name: 'phone',
      testIgnore: [
        'setup.spec.ts',
        'profile.spec.ts',
        'categories.spec.ts',
        'import-commit.spec.ts',
        'accounts.spec.ts',
        'accounts-edit.spec.ts',
        'transactions.spec.ts',
        'undecided.spec.ts',
        'imports.spec.ts',
        'manual-accounts.spec.ts',
        'new-accounts.spec.ts',
        'manual-entry.spec.ts',
        'smoke/**',
      ],
      dependencies: ['categories'],
      use: { ...devices['Pixel 7'], storageState: STORAGE_STATE },
    },
    {
      name: 'desktop',
      testIgnore: [
        'setup.spec.ts',
        'profile.spec.ts',
        'categories.spec.ts',
        'import-commit.spec.ts',
        'accounts.spec.ts',
        'accounts-edit.spec.ts',
        'transactions.spec.ts',
        'undecided.spec.ts',
        'imports.spec.ts',
        'manual-accounts.spec.ts',
        'new-accounts.spec.ts',
        'manual-entry.spec.ts',
        'smoke/**',
      ],
      dependencies: ['categories'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Last, the import itself, once: while it's in place every other upload is refused, and the
    // install has accounts.
    {
      name: 'import-commit',
      testMatch: 'import-commit.spec.ts',
      dependencies: ['phone', 'desktop'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then Accounts, with the imported accounts in it: the test works both widths in turn.
    {
      name: 'accounts',
      testMatch: 'accounts.spec.ts',
      dependencies: ['import-commit'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // And Transactions, with the imported rows in it, on both widths in turn.
    {
      name: 'transactions',
      testMatch: 'transactions.spec.ts',
      dependencies: ['import-commit'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then editing an imported account, once the specs above have read the accounts as imported.
    {
      name: 'accounts-edit',
      testMatch: 'accounts-edit.spec.ts',
      dependencies: ['accounts', 'transactions'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Last, From Actual, undecided: it sets categories on the imported rows, which the specs above
    // read.
    {
      name: 'undecided',
      testMatch: 'undecided.spec.ts',
      dependencies: ['accounts-edit'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then Settings › Imports: it undoes the import and imports again.
    {
      name: 'imports',
      testMatch: 'imports.spec.ts',
      dependencies: ['undecided'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then a manual account, created and given a new balance, after every spec that reads the
    // accounts.
    {
      name: 'manual-accounts',
      testMatch: 'manual-accounts.spec.ts',
      dependencies: ['imports'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then the bank connection: claiming the fake Bridge brings new accounts to Accounts.
    {
      name: 'new-accounts',
      testMatch: 'new-accounts.spec.ts',
      dependencies: ['manual-accounts'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then manual entry, on the manual account above and the imported categories: it works a
    // phone and a desktop in turn.
    {
      name: 'manual-entry',
      testMatch: 'manual-entry.spec.ts',
      dependencies: ['new-accounts'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
  ],
})
